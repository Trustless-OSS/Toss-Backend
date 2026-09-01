//! Worker processors.
//!
//! Every BullMQ worker started in [`crate::infra::queue`] funnels through
//! [`process`], which dispatches on the job name. Processors are the only place
//! background work executes; producers elsewhere in the app just add jobs.

pub mod advance;
pub mod sync;
pub mod webhook;

use tracing::{error, info, warn};

use crate::{
    error::AppError,
    infra::queue::{
        BountyJobData, JOB_ADVANCE_ISSUE, JOB_ESCROW_BALANCE_SYNC, JOB_GITHUB_WEBHOOK,
        JOB_PUSH_MILESTONE, JOB_RELEASE_PAYOUT,
    },
    state::AppState,
};

/// What a processor did.
pub(crate) enum JobOutcome {
    /// Finished; the value becomes the job's return value.
    Done(serde_json::Value),
    /// The processor moved its own job to `delayed` and wants to be re-run later.
    ///
    /// BullMQ treats this as control flow rather than a failure: no attempt is
    /// consumed, and the job keeps its id so a real event can promote it.
    Delayed,
}

/// Route one job to its processor.
pub async fn process(
    state: &AppState,
    mut job: bullmq::Job,
) -> Result<serde_json::Value, bullmq::Error> {
    let name = job.name().to_string();
    let id = job.id().to_string();
    let attempt = job.attempts_made();

    info!(job = %name, job_id = %id, attempt, "processing job");

    let result = match name.as_str() {
        JOB_GITHUB_WEBHOOK => webhook::run(state, &job).await.map(JobOutcome::Done),
        JOB_ADVANCE_ISSUE => {
            let issue_id = payload::<BountyJobData>(&job)
                .ok()
                .map(|data| data.issue_id);
            let result = advance::run_advance(state, &mut job).await;
            if let Some(issue_id) = issue_id {
                if matches!(&result, Ok(JobOutcome::Done(_)) | Ok(JobOutcome::Delayed)) {
                    state.queue.schedule_dirty_drain(issue_id);
                }
            }
            result
        }
        JOB_PUSH_MILESTONE => advance::run_push_milestone(state, &job)
            .await
            .map(JobOutcome::Done),
        JOB_RELEASE_PAYOUT => advance::run_release_payout(state, &job)
            .await
            .map(JobOutcome::Done),
        JOB_ESCROW_BALANCE_SYNC => sync::run(state).await.map(JobOutcome::Done),
        other => {
            warn!(job = other, job_id = %id, "unknown job name ignored");
            Ok(JobOutcome::Done(serde_json::json!({ "skipped": other })))
        }
    };

    match result {
        Ok(JobOutcome::Done(value)) => {
            info!(job = %name, job_id = %id, attempt, "job completed");
            Ok(value)
        }
        Ok(JobOutcome::Delayed) => {
            info!(job = %name, job_id = %id, "job parked; it keeps its id and can be promoted");
            Err(bullmq::Error::Delayed)
        }
        Err(error) => {
            let job_error = to_job_error(error);
            if matches!(job_error, bullmq::Error::Unrecoverable(_)) {
                error!(job = %name, job_id = %id, error = %job_error, "job failed permanently");
            } else {
                warn!(job = %name, job_id = %id, attempt, error = %job_error, "job failed; will retry with backoff");
            }
            Err(job_error)
        }
    }
}

/// Classify an application error for BullMQ's retry machinery.
///
/// Transient faults — Trustless Work timeouts, GitHub 5xx and rate limits,
/// database blips — become retryable so the queue backs off and tries again.
/// Errors that describe a state which cannot change on retry (a malformed
/// payload, a missing record, a rejected request) are marked unrecoverable so
/// they fail once and land in `failed` with their reason intact.
fn to_job_error(error: AppError) -> bullmq::Error {
    match error {
        AppError::BadRequest { .. }
        | AppError::Unauthorized { .. }
        | AppError::Forbidden { .. }
        | AppError::NotFound { .. }
        | AppError::WebhookError { .. }
        | AppError::EnvVarError { .. } => bullmq::Error::Unrecoverable(error.to_string()),
        AppError::StellarError { .. }
        | AppError::GitHubError { .. }
        | AppError::DatabaseError { .. }
        | AppError::Internal { .. } => bullmq::Error::ProcessingError(error.to_string()),
    }
}

/// Deserialize a job body, reporting a bad payload as unrecoverable.
pub(crate) fn payload<T: serde::de::DeserializeOwned>(job: &bullmq::Job) -> Result<T, AppError> {
    serde_json::from_value(job.data().clone())
        .map_err(|error| AppError::webhook(format!("invalid job payload: {error}")))
}

/// Surface a queue-level failure as a retryable application error.
pub(crate) fn queue_error(error: bullmq::Error) -> AppError {
    AppError::internal(format!("queue error: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permanent_failures_are_not_retried() {
        assert!(matches!(
            to_job_error(AppError::bad_request("no escrow deployed")),
            bullmq::Error::Unrecoverable(_)
        ));
        assert!(matches!(
            to_job_error(AppError::webhook("payload missing repository")),
            bullmq::Error::Unrecoverable(_)
        ));
    }

    #[test]
    fn transient_failures_are_retried() {
        assert!(matches!(
            to_job_error(AppError::internal("TrustlessWork request failed: timeout")),
            bullmq::Error::ProcessingError(_)
        ));
        assert!(matches!(
            to_job_error(AppError::github("502 Bad Gateway")),
            bullmq::Error::ProcessingError(_)
        ));
        assert!(matches!(
            to_job_error(AppError::database("connection reset")),
            bullmq::Error::ProcessingError(_)
        ));
    }

    #[test]
    fn the_failure_reason_survives_classification() {
        let error = to_job_error(AppError::internal("TrustlessWork 503"));
        assert!(error.to_string().contains("TrustlessWork 503"));
    }
}
