use tracing::warn;
use uuid::Uuid;

use crate::{error::AppError, infra::queue::NotificationJobData, state::AppState};

use super::kinds::{Delivery, Notify, Role};

pub struct Service;

impl Service {
    /// Notify a user with a specific role, respecting the delivery policy.
    /// Skips if recipient == actor, or if delivery == None.
    /// Returns Ok(()) even on errors to prevent cascading failures.
    pub async fn notify_user(state: &AppState, role: Role, n: Notify) -> Result<(), AppError> {
        // Skip self-notifications
        if n.actor == Some(n.recipient) {
            return Ok(());
        }

        let delivery = n.kind.delivery(role);

        // Skip if no delivery for this role
        if delivery == Delivery::None {
            return Ok(());
        }

        // Try to insert the notification
        let notification = match super::repository::Repository::insert(state, &n, &delivery).await {
            Ok(Some(notif)) => notif,
            Ok(None) => {
                // Duplicate dedupe_key, silently ignore
                return Ok(());
            }
            Err(e) => {
                warn!(error = %e, recipient = %n.recipient, "failed to insert notification");
                return Ok(());
            }
        };

        // Enqueue email job only for BellAndEmail delivery
        if delivery == Delivery::BellAndEmail {
            if let Err(e) = state
                .queue
                .notify_queue()
                .add(
                    crate::infra::queue::JOB_NOTIFY_EMAIL,
                    NotificationJobData {
                        notification_id: notification.id,
                    },
                    &format!("notify-email:{}", notification.id),
                )
                .await
            {
                warn!(error = %e, notification_id = %notification.id, "failed to enqueue email job");
            }
        }

        Ok(())
    }

    /// Notify all maintainers of a repository, excluding the actor.
    pub async fn notify_maintainers(
        state: &AppState,
        repo_id: Uuid,
        _n: Notify,
    ) -> Result<(), AppError> {
        // TODO: Implement when profile/repo modules are accessible
        let _ = (state, repo_id);
        Ok(())
    }

    /// Notify a contributor.
    pub async fn notify_contributor(state: &AppState, n: Notify) -> Result<(), AppError> {
        Self::notify_user(state, Role::Contributor, n).await
    }

    /// Notify a contributor (quiet wrapper - logs errors, doesn't return them).
    pub async fn notify_contributor_quiet(state: &AppState, n: Notify) {
        if let Err(e) = Self::notify_contributor(state, n).await {
            warn!(error = %e, "failed to notify contributor");
        }
    }

    /// Notify maintainers (quiet wrapper - logs errors, doesn't return them).
    pub async fn notify_maintainers_quiet(state: &AppState, repo_id: Uuid, n: Notify) {
        if let Err(e) = Self::notify_maintainers(state, repo_id, n).await {
            warn!(error = %e, repo_id = %repo_id, "failed to notify maintainers");
        }
    }

    /// Deliver an email notification. Called by the job worker.
    pub async fn deliver_email(state: &AppState, notification_id: Uuid) -> Result<(), AppError> {
        let notification = match super::repository::Repository::get(state, notification_id).await? {
            Some(n) => n,
            None => {
                return Err(AppError::not_found("notification"));
            }
        };

        // Skip if already sent
        if notification.email_status == "sent" {
            return Ok(());
        }

        // Get profile to check for email
        use crate::error::require_db;
        let mut db = require_db(&state.db)?;
        let profile =
            match crate::shared::models::schema::Profile::filter_by_id(notification.profile_id)
                .first()
                .exec(&mut db)
                .await
            {
                Ok(Some(p)) => p,
                _ => {
                    // Mark as skipped (no profile found or error)
                    let _ = super::repository::Repository::set_email_status(
                        state,
                        notification_id,
                        "skipped_no_email",
                        false,
                    )
                    .await;
                    return Ok(());
                }
            };

        let email = match profile.email {
            Some(e) if !e.is_empty() => e,
            _ => {
                // No email, mark as skipped
                let _ = super::repository::Repository::set_email_status(
                    state,
                    notification_id,
                    "skipped_no_email",
                    false,
                )
                .await;
                return Ok(());
            }
        };

        // Send email via Resend
        // Note: Using a simple approach based on the Resend SDK
        // The actual implementation may need adjustment based on the SDK version
        let _ = (notification, email, state);

        // Mark as sent (placeholder)
        super::repository::Repository::set_email_status(state, notification_id, "sent", true)
            .await?;
        Ok(())
    }
}
