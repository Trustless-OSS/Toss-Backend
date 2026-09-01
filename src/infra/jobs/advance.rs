//! The bounty automation processors: `advance-issue`, `push-milestone` and
//! `release-payout`.
//!
//! `advance-issue` decides; the other two act. All three re-read Postgres,
//! GitHub and the escrow contract before doing anything, so a job that has been
//! sitting in the queue — or retrying with backoff — can never act on stale
//! state.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tracing::{info, warn};

use crate::{
    error::AppError,
    infra::{
        jobs::{payload, queue_error, JobOutcome},
        queue::BountyJobData,
    },
    modules::bounty::automation::{self, Decision, IssueContext},
    state::AppState,
};

/// How many times one `advance-issue` job re-evaluates before yielding.
///
/// A pass is repeated only when new events landed while it was running, so this
/// bound is a safety valve rather than a normal path.
const MAX_PASSES: u32 = 3;

/// How many times an issue parks itself on a timer before it stops revisiting.
/// Real events still wake it at any point.
const MAX_RECHECKS: u32 = 12;

const RECHECK_BASE: Duration = Duration::from_secs(60);
const RECHECK_CAP: Duration = Duration::from_secs(30 * 60);

/// Evaluate an issue and take the next allowed step.
pub(crate) async fn run_advance(
    state: &AppState,
    job: &mut bullmq::Job,
) -> Result<JobOutcome, AppError> {
    let data: BountyJobData = payload(job)?;
    let issue_id = data.issue_id;

    if data.exhausted() {
        warn!(
            %issue_id,
            hops = data.hops,
            "handoff budget exhausted; stopping instead of looping"
        );
        state.queue.schedule_dirty_drain(issue_id);
        return Ok(JobOutcome::Done(
            serde_json::json!({ "skipped": "hop-budget-exhausted" }),
        ));
    }

    let mut decisions: Vec<&'static str> = Vec::new();
    let mut park_reason: Option<String> = None;
    let mut issue_number = 0;

    for pass in 0..MAX_PASSES {
        // Clear before reading: anything that arrives from here on re-sets the
        // flag and earns another pass.
        state.queue.clear_dirty(issue_id).await?;

        let Some(ctx) = automation::load_context(state, issue_id).await? else {
            info!(%issue_id, "issue no longer exists; advance job is a no-op");
            state.queue.schedule_dirty_drain(issue_id);
            return Ok(JobOutcome::Done(
                serde_json::json!({ "skipped": "issue-not-found" }),
            ));
        };
        issue_number = ctx.issue.github_issue_number;

        let decision = automation::evaluate(state, &ctx).await?;
        info!(
            %issue_id,
            issue = issue_number,
            trigger = %data.trigger,
            pass,
            decision = decision.label(),
            "issue evaluated"
        );
        decisions.push(decision.label());

        apply(state, &ctx, &decision, &data).await?;

        // The last pass decides whether the issue is parked; an event arriving
        // mid-job can turn a "waiting" into a "release".
        park_reason = decision.park_reason();

        if !state.queue.is_dirty(issue_id).await? {
            break;
        }
        info!(%issue_id, "new events arrived mid-job; re-evaluating");
    }

    if let Some(reason) = park_reason {
        if park(job, &data, &reason, issue_id, issue_number).await? {
            // The job is already delayed, so a dirty flag can be turned into a
            // promote instead of waiting for the timer.
            if let Err(error) = state.queue.drain_dirty_advance(issue_id).await {
                warn!(%error, %issue_id, "failed to drain dirty flag after parking");
            }
            return Ok(JobOutcome::Delayed);
        }
    }

    Ok(JobOutcome::Done(serde_json::json!({
        "issueId": issue_id,
        "decisions": decisions,
    })))
}

/// Act on a decision by queueing the concrete step.
///
/// Nothing here touches the chain directly: money-moving work always goes back
/// through the queue so it gets its own retries and its own per-issue job id.
async fn apply(
    state: &AppState,
    ctx: &IssueContext,
    decision: &Decision,
    data: &BountyJobData,
) -> Result<(), AppError> {
    let issue_id = ctx.issue.id;

    match decision {
        Decision::PushMilestone { .. } => {
            let outcome = state
                .queue
                .enqueue_push_milestone(data.next("rules-passed"))
                .await?;
            info!(%issue_id, outcome = outcome.label(), "milestone push queued");
        }

        Decision::ReleasePayout { .. } => {
            let outcome = state
                .queue
                .enqueue_release_payout(data.next("rules-passed"))
                .await?;
            info!(%issue_id, outcome = outcome.label(), "payout queued");
        }

        Decision::RepairDatabase { milestone_index } => {
            automation::repair_database(state, ctx, *milestone_index).await?;
        }

        Decision::WaitForWallet { github_username } => {
            // Tell the contributor once, on the event that actually parked the
            // payout — never again on the timed re-checks.
            if data.notify && data.recheck == 0 {
                if let Err(error) =
                    automation::notify_waiting_for_wallet(state, ctx, github_username).await
                {
                    warn!(%error, "failed to comment about the missing wallet");
                }
            }
        }

        Decision::Waiting { .. } => {}

        Decision::Settled => {
            info!(
                %issue_id,
                issue = ctx.issue.github_issue_number,
                "issue is fully settled; nothing to do"
            );
        }

        Decision::Blocked { reason } => {
            // Deliberately terminal: a blocked rule means the live state does not
            // justify moving money, and retrying it on a timer would be exactly
            // the wrong behaviour.
            warn!(
                %issue_id,
                issue = ctx.issue.github_issue_number,
                reason,
                "automation blocked; funds were not moved"
            );
        }
    }

    Ok(())
}

/// Park this job on a timer, keeping its id.
///
/// The job moves *itself* to `delayed` rather than enqueueing a second job —
/// a running job already owns `advance-issue:<issue-id>`, so re-adding it would
/// be swallowed as a duplicate. Keeping the id is also what lets a wallet
/// connect or a merged PR **promote** the parked job and resume immediately;
/// the timer is only the backstop.
///
/// Returns `false` when the re-check budget is spent, leaving the job to
/// complete normally and wait for a real event.
async fn park(
    job: &mut bullmq::Job,
    data: &BountyJobData,
    reason: &str,
    issue_id: uuid::Uuid,
    issue_number: i32,
) -> Result<bool, AppError> {
    let next = data.recheck + 1;

    if next > MAX_RECHECKS {
        info!(
            %issue_id,
            issue = issue_number,
            reason,
            "re-check budget exhausted; waiting for the next real event"
        );
        return Ok(false);
    }

    let delay = recheck_delay(data.recheck);

    // Persist the widened backoff before parking, so each visit waits longer.
    let updated = BountyJobData::new(issue_id, "scheduled-recheck").with_recheck(next);
    job.update_data(
        serde_json::to_value(&updated).map_err(|error| AppError::internal(error.to_string()))?,
    )
    .await
    .map_err(queue_error)?;

    let resume_at = now_millis() + delay.as_millis() as u64;
    job.move_to_delayed(resume_at).await.map_err(queue_error)?;

    info!(
        %issue_id,
        issue = issue_number,
        reason,
        recheck = next,
        delay_secs = delay.as_secs(),
        "flow parked; job delayed and promotable"
    );

    Ok(true)
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

fn recheck_delay(recheck: u32) -> Duration {
    let multiplier = 1u64.checked_shl(recheck.min(16)).unwrap_or(u64::MAX);
    RECHECK_BASE
        .saturating_mul(multiplier.min(u32::MAX as u64) as u32)
        .min(RECHECK_CAP)
}

/// Push the milestone on-chain, once the live rules still ask for it.
pub(crate) async fn run_push_milestone(
    state: &AppState,
    job: &bullmq::Job,
) -> Result<serde_json::Value, AppError> {
    let data: BountyJobData = payload(job)?;

    if data.exhausted() {
        warn!(
            issue_id = %data.issue_id,
            hops = data.hops,
            "handoff budget exhausted; refusing to push again"
        );
        return Ok(serde_json::json!({ "skipped": "hop-budget-exhausted" }));
    }

    let Some(ctx) = automation::load_context(state, data.issue_id).await? else {
        return Ok(serde_json::json!({ "skipped": "issue-not-found" }));
    };

    match automation::evaluate(state, &ctx).await? {
        Decision::PushMilestone {
            payout_address,
            payout_chain,
        } => {
            let milestone_index =
                automation::push_milestone(state, &ctx, &payout_address, &payout_chain).await?;

            // Keep the chain moving: the next evaluation may already be able to
            // release, for example when the PR was merged before the wallet
            // arrived.
            state
                .queue
                .enqueue_advance_issue(data.next("milestone-pushed"))
                .await?;

            Ok(serde_json::json!({ "milestoneIndex": milestone_index }))
        }
        other => {
            info!(
                issue_id = %data.issue_id,
                decision = other.label(),
                "rules no longer call for a milestone push"
            );
            Ok(serde_json::json!({ "skipped": other.label() }))
        }
    }
}

/// Release the bounty, once the live rules still ask for it.
///
/// This is the only place funds leave escrow on the happy path, and it re-reads
/// every source of truth immediately beforehand.
pub(crate) async fn run_release_payout(
    state: &AppState,
    job: &bullmq::Job,
) -> Result<serde_json::Value, AppError> {
    let data: BountyJobData = payload(job)?;

    let Some(ctx) = automation::load_context(state, data.issue_id).await? else {
        return Ok(serde_json::json!({ "skipped": "issue-not-found" }));
    };

    match automation::evaluate(state, &ctx).await? {
        Decision::ReleasePayout {
            milestone_index,
            split_percentage,
        } => {
            automation::release_payout(state, &ctx, milestone_index, split_percentage).await?;
            Ok(serde_json::json!({
                "released": true,
                "milestoneIndex": milestone_index,
                "splitPercentage": split_percentage,
            }))
        }
        Decision::RepairDatabase { milestone_index } => {
            // The chain paid this milestone already — most likely on a previous
            // attempt that failed after the transaction landed. Fix the books,
            // move no money.
            automation::repair_database(state, &ctx, milestone_index).await?;
            Ok(serde_json::json!({ "repaired": true }))
        }
        other => {
            warn!(
                issue_id = %data.issue_id,
                decision = other.label(),
                "payout job stood down: the live rules no longer authorise a release"
            );
            Ok(serde_json::json!({ "skipped": other.label() }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recheck_delay_backs_off_and_is_capped() {
        assert_eq!(recheck_delay(0), RECHECK_BASE);
        assert_eq!(recheck_delay(1), RECHECK_BASE * 2);
        assert_eq!(recheck_delay(3), RECHECK_BASE * 8);
        assert_eq!(recheck_delay(MAX_RECHECKS), RECHECK_CAP);
        assert_eq!(recheck_delay(u32::MAX), RECHECK_CAP);
    }

    #[test]
    fn recheck_delay_never_exceeds_the_cap() {
        for recheck in 0..64 {
            assert!(recheck_delay(recheck) <= RECHECK_CAP);
        }
    }

    #[test]
    fn only_parking_decisions_produce_a_park_reason() {
        // Mirrors the match in `run_advance`: acting decisions must not park.
        for decision in [
            Decision::Settled,
            Decision::Blocked {
                reason: "receiver mismatch".to_string(),
            },
            Decision::ReleasePayout {
                milestone_index: 0,
                split_percentage: None,
            },
            Decision::RepairDatabase { milestone_index: 0 },
        ] {
            assert!(
                !decision.wants_recheck(),
                "{} should not park",
                decision.label()
            );
        }
    }
}
