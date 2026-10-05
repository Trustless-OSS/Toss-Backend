use tracing::warn;
use uuid::Uuid;

use crate::{error::AppError, infra::queue::NotificationJobData, state::AppState};

use super::kinds::{Delivery, Notify, Role};

pub struct Service;

impl Service {
    /// Get actor's profile ID if present.
    async fn actor_profile_id(state: &AppState, github_id: i64) -> Option<Uuid> {
        crate::modules::contributor::repository::get_profile_by_github_id(state, github_id)
            .await
            .ok()
            .flatten()
            .map(|p| p.id)
    }

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

        // Enqueue or spawn email job for BellAndEmail delivery
        if delivery == Delivery::BellAndEmail {
            let notification_id = notification.id;

            // Try to enqueue; if queue is disabled or enqueue fails, spawn async delivery
            let enqueue_result = if state.queue.is_enabled() {
                state
                    .queue
                    .notify_queue()
                    .add(
                        crate::infra::queue::JOB_NOTIFY_EMAIL,
                        NotificationJobData { notification_id },
                        &format!("notify-email:{}", notification_id),
                    )
                    .await
            } else {
                Err(AppError::internal("queue disabled"))
            };

            if let Err(e) = enqueue_result {
                warn!(error = %e, notification_id = %notification_id, "queue enqueue failed; spawning direct delivery");
                let state_clone = state.clone();
                tokio::spawn(async move {
                    if let Err(e) = Self::deliver_email(&state_clone, notification_id).await {
                        warn!(error = %e, notification_id = %notification_id, "direct email delivery failed");
                    }
                });
            }
        }

        Ok(())
    }

    /// Notify all maintainers of a repository, excluding the actor.
    pub async fn notify_maintainers(
        state: &AppState,
        repo_id: Uuid,
        n: Notify,
    ) -> Result<(), AppError> {
        let maintainers =
            match crate::shared::models::schema::RepoMaintainer::filter_by_repo_id(repo_id)
                .exec(&mut crate::error::require_db(&state.db)?)
                .await
            {
                Ok(rows) => rows,
                Err(e) => {
                    warn!(error = %e, repo_id = %repo_id, "failed to fetch maintainers");
                    return Ok(());
                }
            };

        for maintainer_row in maintainers {
            // Skip if maintainer is the actor
            if n.actor == Some(maintainer_row.profile_id) {
                continue;
            }

            let mut notif = n.clone();
            notif.recipient = maintainer_row.profile_id;
            notif.dedupe_key = format!("{}:{}", n.dedupe_key, maintainer_row.profile_id);

            if let Err(e) = Self::notify_user(state, Role::Maintainer, notif).await {
                warn!(error = %e, profile_id = %maintainer_row.profile_id, "failed to notify maintainer");
            }
        }

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
                // Try to fetch from GitHub API as fallback
                match fetch_github_user_email(state, profile.github_id).await {
                    Ok(Some(e)) => {
                        // Cache it for future use
                        let _ = crate::modules::contributor::repository::set_profile_email(
                            state,
                            profile.github_id,
                            &e,
                        )
                        .await;
                        e
                    }
                    _ => {
                        // No email found, mark as skipped
                        let _ = super::repository::Repository::set_email_status(
                            state,
                            notification_id,
                            "skipped_no_email",
                            false,
                        )
                        .await;
                        return Ok(());
                    }
                }
            }
        };

        // Get email template for this notification type
        let kind_str = notification.kind.as_str();
        let kind =
            super::kinds::Kind::from_str(kind_str).unwrap_or(super::kinds::Kind::BountyAssigned); // Fallback to default

        let template = crate::modules::notification::email_templates::email_template_for(
            kind,
            &notification.title,
            &notification.body.as_deref().unwrap_or(""),
            &notification.data.clone().unwrap_or(serde_json::json!({})),
            &state.config.app_url,
        );

        // Send email via Resend
        use resend_rs::types::CreateEmailBaseOptions;
        let msg =
            CreateEmailBaseOptions::new(&state.config.email_from, [&email], &template.subject)
                .with_html(&template.html);

        state
            .email
            .emails
            .send(msg)
            .await
            .map_err(|e| AppError::internal(format!("resend: {e}")))?;

        // Mark as sent
        super::repository::Repository::set_email_status(state, notification_id, "sent", true)
            .await?;
        Ok(())
    }
}

/// Try to fetch user's email from GitHub API
async fn fetch_github_user_email(
    state: &AppState,
    github_id: i64,
) -> Result<Option<String>, AppError> {
    let url = format!("https://api.github.com/user/{}", github_id);

    match state
        .http_client
        .get(&url)
        .header("Accept", "application/vnd.github.v3+json")
        .header("User-Agent", "trustless-oss")
        .send()
        .await
    {
        Ok(response) => match response.json::<serde_json::Value>().await {
            Ok(body) => Ok(body
                .get("email")
                .and_then(|e| e.as_str())
                .filter(|e| !e.is_empty())
                .map(|e| e.to_string())),
            Err(_) => Ok(None),
        },
        Err(_) => Ok(None),
    }
}
