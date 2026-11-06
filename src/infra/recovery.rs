//! Durable recovery for high-value jobs. Redis executes work; Postgres retains failures.
use crate::{
    error::{map_db_err, require_db, AppError},
    infra::queue::{
        BountyJobData, WebhookJobData, JOB_ADVANCE_ISSUE, JOB_GITHUB_WEBHOOK, JOB_PUSH_MILESTONE,
        JOB_RELEASE_PAYOUT,
    },
    shared::models::schema,
    state::AppState,
};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub const FUNDING_WAIT: &str = "waiting-for-escrow-funding";
pub fn waiting_for_funds() -> AppError {
    AppError::internal(FUNDING_WAIT)
}

pub(crate) fn accepts_recovered_webhook(status: &str) -> bool {
    matches!(status, "pending" | "waiting_funding")
}

fn task_id(name: &str, id: &str) -> Uuid {
    let hash = Sha256::digest(format!("{name}/{id}"));
    let mut bytes = [0; 16];
    bytes.copy_from_slice(&hash[..16]);
    Uuid::from_bytes(bytes)
}

pub async fn record_failure(
    state: &AppState,
    name: &str,
    id: &str,
    data: &serde_json::Value,
    error: &str,
) -> Result<(), AppError> {
    if !matches!(
        name,
        JOB_GITHUB_WEBHOOK | JOB_ADVANCE_ISSUE | JOB_PUSH_MILESTONE | JOB_RELEASE_PAYOUT
    ) {
        return Ok(());
    }
    let mut db = require_db(&state.db)?;
    let task_id = data
        .pointer("/payload/_recoveryTaskId")
        .and_then(|v| v.as_str())
        .and_then(|id| Uuid::parse_str(id).ok())
        .unwrap_or_else(|| task_id(name, id));
    let previous = schema::FailedTask::filter_by_id(task_id)
        .first()
        .exec(&mut db)
        .await
        .map_err(map_db_err)?;
    let attempts = previous
        .as_ref()
        .map_or(1, |task| task.attempts.saturating_add(1));
    let funding = error.contains(FUNDING_WAIT);
    // Unexpected webhook failures are retained for inspection, never blindly replayed.
    let status = failure_status(name, funding, attempts);
    schema::FailedTask::upsert_by_id(task_id)
        .task_type(name.to_string())
        .ref_id(
            serde_json::from_value::<BountyJobData>(data.clone())
                .ok()
                .map(|job| job.issue_id),
        )
        .payload(serde_json::json!({"jobId": id, "data": data}))
        .error(Some(error.to_string()))
        .attempts(attempts)
        .status(status.to_string())
        .last_attempt_at(Some(jiff::Timestamp::now()))
        .exec(&mut db)
        .await
        .map_err(map_db_err)?;
    Ok(())
}

pub async fn resolve(state: &AppState, name: &str, id: &str) -> Result<(), AppError> {
    if !matches!(
        name,
        JOB_GITHUB_WEBHOOK | JOB_ADVANCE_ISSUE | JOB_PUSH_MILESTONE | JOB_RELEASE_PAYOUT
    ) {
        return Ok(());
    }
    let mut db = require_db(&state.db)?;
    toasty::update!(schema::FailedTask::filter_by_id(task_id(name, id)) { status: "resolved".to_string() })
        .exec(&mut db).await.map_err(map_db_err)?;
    Ok(())
}

pub async fn recover(state: &AppState) -> Result<(), AppError> {
    if !state.queue.is_enabled() {
        return Ok(());
    }
    let mut db = require_db(&state.db)?;
    // Oldest first and bounded: retries cannot monopolize the API or the worker.
    let tasks = schema::FailedTask::filter(schema::FailedTask::fields().status().eq("pending"))
        .order_by(schema::FailedTask::fields().last_attempt_at().asc())
        .limit(100)
        .exec(&mut db)
        .await
        .map_err(map_db_err)?;
    for task in tasks {
        // Park funding waits written by older versions without replaying their comments.
        if task.task_type == JOB_GITHUB_WEBHOOK
            && task
                .error
                .as_deref()
                .is_some_and(|error| error.contains(FUNDING_WAIT))
        {
            toasty::update!(schema::FailedTask::filter_by_id(task.id) { status: "waiting_funding".to_string() })
                .exec(&mut db).await.map_err(map_db_err)?;
            continue;
        }
        if task
            .last_attempt_at
            .is_some_and(|at| jiff::Timestamp::now().as_second() - at.as_second() < 60)
        {
            continue;
        }
        if let Err(error) = recover_task(state, &task).await {
            tracing::warn!(%error, task_id = %task.id, "durable task recovery failed");
        }
        // Rotate even on an unavailable external service, avoiding starvation.
        toasty::update!(schema::FailedTask::filter_by_id(task.id) { last_attempt_at: Some(jiff::Timestamp::now()) })
            .exec(&mut db).await.map_err(map_db_err)?;
    }
    Ok(())
}

async fn recover_task(state: &AppState, task: &schema::FailedTask) -> Result<(), AppError> {
    let id = task.payload["jobId"]
        .as_str()
        .ok_or_else(|| AppError::internal("recovery job ID missing"))?;
    if task.task_type == JOB_GITHUB_WEBHOOK
        && task
            .error
            .as_deref()
            .is_some_and(|error| error.contains(FUNDING_WAIT))
    {
        let mut data: WebhookJobData = serde_json::from_value(task.payload["data"].clone())
            .map_err(|e| AppError::internal(e.to_string()))?;
        let repo_id = data.payload["repository"]["id"]
            .as_i64()
            .ok_or_else(|| AppError::internal("recovery repository missing"))?;
        let Some(repo) =
            crate::modules::repo::repository::get_repo_by_github_id(state, repo_id).await?
        else {
            let mut db = require_db(&state.db)?;
            toasty::update!(schema::FailedTask::filter_by_id(task.id) { status: "resolved".to_string() }).exec(&mut db).await.map_err(map_db_err)?;
            return Ok(());
        };
        let number = data.payload["issue"]["number"]
            .as_i64()
            .ok_or_else(|| AppError::internal("recovery issue missing"))?
            as i32;
        data.payload["_recoveryTaskId"] = serde_json::json!(task.id);
        data.payload["issue"] = crate::modules::github::auth::fetch_issue_payload(
            state,
            repo_id,
            &repo.full_name,
            number,
        )
        .await?;
        data.payload["repository"]["full_name"] = serde_json::json!(repo.full_name);
        // Refresh labels and assignees instead of trusting the old event snapshot.
        if data.event == "issues" {
            data.action = Some("opened".to_string());
            data.payload["action"] = serde_json::json!("opened");
            data.payload["label"] = serde_json::Value::Null;
        }
        state
            .queue
            .enqueue_webhook(Some(id.strip_prefix("github-webhook:").unwrap_or(id)), data)
            .await?;
    } else if let Some(issue_id) = task.ref_id {
        let data = BountyJobData::new(issue_id, "durable-recovery");
        // Re-evaluate all financial preconditions inside the existing workers.
        match task.task_type.as_str() {
            JOB_PUSH_MILESTONE => {
                state.queue.enqueue_push_milestone(data).await?;
            }
            JOB_RELEASE_PAYOUT => {
                state.queue.enqueue_release_payout(data).await?;
            }
            JOB_ADVANCE_ISSUE => {
                state.queue.enqueue_advance_issue(data).await?;
            }
            _ => {}
        }
    }
    Ok(())
}

/// Reconcile missed wallet/funding/merge events using indexed, paginated reads.
pub async fn reconcile_assigned(state: &AppState) -> Result<(), AppError> {
    if !state.queue.is_enabled() {
        return Ok(());
    }
    let mut db = require_db(&state.db)?;
    let dead_refs: std::collections::HashSet<Uuid> =
        schema::FailedTask::filter(schema::FailedTask::fields().status().eq("dead"))
            .exec(&mut db)
            .await
            .map_err(map_db_err)?
            .into_iter()
            .filter_map(|task| task.ref_id)
            .collect();
    let mut offset = 0;
    loop {
        let bounties = schema::Bounty::filter(
            schema::Bounty::fields()
                .status()
                .eq("assigned")
                .or(schema::Bounty::fields().status().eq("merged")),
        )
        .order_by(schema::Bounty::fields().id().asc())
        .limit(100)
        .offset(offset)
        .exec(&mut db)
        .await
        .map_err(map_db_err)?;
        let count = bounties.len();
        for bounty in bounties {
            // Explicitly dead financial tasks require operator review.
            if dead_refs.contains(&bounty.id) {
                continue;
            }
            state
                .queue
                .enqueue_advance_issue(BountyJobData::new(bounty.id, "reconciliation"))
                .await?;
        }
        if count < 100 {
            break;
        }
        offset += count;
    }
    Ok(())
}

pub fn start(state: AppState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut tick = 0u64;
        loop {
            interval.tick().await;
            if let Err(error) = recover(&state).await {
                tracing::warn!(%error, "recovery scan failed");
            }
            if tick.is_multiple_of(5) {
                if let Err(error) = reconcile_assigned(&state).await {
                    tracing::warn!(%error, "bounty reconciliation failed");
                }
            }
            tick = tick.wrapping_add(1);
        }
    })
}

pub async fn resolve_recovered_webhook(
    state: &AppState,
    data: &serde_json::Value,
) -> Result<(), AppError> {
    let Some(id) = data
        .pointer("/payload/_recoveryTaskId")
        .and_then(|v| v.as_str())
        .and_then(|id| Uuid::parse_str(id).ok())
    else {
        return Ok(());
    };
    let mut db = require_db(&state.db)?;
    toasty::update!(schema::FailedTask::filter_by_id(id) { status: "resolved".to_string() })
        .exec(&mut db)
        .await
        .map_err(map_db_err)?;
    Ok(())
}

/// A new maintainer request supersedes a parked amount for the same GitHub issue.
pub async fn supersede_funding(
    state: &AppState,
    payload: &serde_json::Value,
) -> Result<(), AppError> {
    if payload.get("_recoveryTaskId").is_some() {
        return Ok(());
    }
    let mut db = require_db(&state.db)?;
    let tasks = schema::FailedTask::filter_by_task_type(JOB_GITHUB_WEBHOOK.to_string())
        .filter(
            schema::FailedTask::fields()
                .status()
                .eq("pending")
                .or(schema::FailedTask::fields().status().eq("waiting_funding")),
        )
        .exec(&mut db)
        .await
        .map_err(map_db_err)?;
    for task in tasks {
        let old = &task.payload["data"]["payload"];
        if task
            .error
            .as_deref()
            .is_some_and(|error| error.contains(FUNDING_WAIT))
            && old["repository"]["id"] == payload["repository"]["id"]
            && old["issue"]["id"] == payload["issue"]["id"]
        {
            toasty::update!(schema::FailedTask::filter_by_id(task.id) { status: "resolved".to_string() })
                .exec(&mut db).await.map_err(map_db_err)?;
        }
    }
    Ok(())
}

pub async fn wake_repo(state: &AppState, repo_id: Uuid) -> Result<(), AppError> {
    let mut db = require_db(&state.db)?;
    let bounties = schema::Bounty::filter_by_repo_id(repo_id)
        .exec(&mut db)
        .await
        .map_err(map_db_err)?;
    for bounty in bounties {
        if matches!(bounty.status.as_str(), "assigned" | "merged") {
            state
                .queue
                .enqueue_advance_issue(BountyJobData::new(bounty.id, "prerequisite-ready"))
                .await?;
        }
    }
    let tasks = schema::FailedTask::filter_by_task_type(JOB_GITHUB_WEBHOOK.to_string())
        .filter(
            schema::FailedTask::fields()
                .status()
                .eq("pending")
                .or(schema::FailedTask::fields().status().eq("waiting_funding")),
        )
        .exec(&mut db)
        .await
        .map_err(map_db_err)?;
    let repo = crate::modules::repo::repository::get_repo_by_id(state, repo_id).await?;
    if let Some(repo) = repo {
        for task in tasks {
            if task.payload["data"]["payload"]["repository"]["id"].as_i64()
                == Some(repo.github_repo_id)
                && task
                    .error
                    .as_deref()
                    .is_some_and(|error| error.contains(FUNDING_WAIT))
            {
                recover_task(state, &task).await?;
            }
        }
    }
    Ok(())
}

fn failure_status(name: &str, funding: bool, attempts: i32) -> &'static str {
    if funding && name == JOB_GITHUB_WEBHOOK {
        "waiting_funding"
    } else if !funding && (name == JOB_GITHUB_WEBHOOK || attempts >= 20) {
        "dead"
    } else {
        "pending"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn funding_wakeups_can_run_but_superseded_requests_cannot() {
        assert!(accepts_recovered_webhook("waiting_funding"));
        assert!(accepts_recovered_webhook("pending"));
        assert!(!accepts_recovered_webhook("resolved"));
        assert!(!accepts_recovered_webhook("dead"));
    }

    #[test]
    fn funding_webhooks_are_parked_and_payment_retries_remain_bounded() {
        assert_eq!(failure_status(JOB_RELEASE_PAYOUT, false, 19), "pending");
        assert_eq!(failure_status(JOB_RELEASE_PAYOUT, false, 20), "dead");
        for attempts in [1, 20, 100] {
            assert_eq!(
                failure_status(JOB_GITHUB_WEBHOOK, true, attempts),
                "waiting_funding"
            );
        }
        assert_eq!(failure_status(JOB_ADVANCE_ISSUE, true, 1), "pending");
    }

    #[test]
    fn stale_webhooks_are_not_blindly_replayed() {
        assert_eq!(failure_status(JOB_GITHUB_WEBHOOK, false, 1), "dead");
        assert_ne!(
            task_id(JOB_RELEASE_PAYOUT, "same-id"),
            task_id(JOB_PUSH_MILESTONE, "same-id")
        );
        assert_eq!(
            task_id(JOB_RELEASE_PAYOUT, "same-id"),
            task_id(JOB_RELEASE_PAYOUT, "same-id")
        );
    }
}
