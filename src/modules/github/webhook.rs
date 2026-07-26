use serde::Deserialize;
use tracing::{error, info};

use crate::{
    error::AppError,
    infra::queue::WebhookJobEnvelope,
    modules::github::handlers::{
        handle_issue_assigned, handle_issue_closed, handle_issue_comment_created,
        handle_issue_deleted, handle_issue_labeled, handle_issue_unassigned, handle_pr_merged,
    },
    modules::github::idempotency::{self, DeliveryClaim},
    modules::repo::repository::{
        delete_repo_by_github_id, delete_repos_by_installation_id, upsert_installation_repo,
    },
    state::AppState,
};

#[derive(Debug, Deserialize)]
struct GithubInstallation {
    id: i64,
    account: GithubAccount,
}

#[derive(Debug, Deserialize)]
struct GithubAccount {
    id: i64,
    login: String,
    #[serde(rename = "type")]
    account_type: String,
}

#[derive(Debug, Deserialize)]
struct GithubRepository {
    id: i64,
    full_name: String,
    private: bool,
    fork: bool,
}

#[derive(Debug, Deserialize)]
struct GithubSender {
    id: i64,
}

/// Entry point for both the queue worker and the synchronous
/// (`installation*`) fast path. Guards every delivery-id-bearing event
/// through the Postgres idempotency ledger before dispatching, and records
/// the outcome back onto it, so a redelivered event never re-runs a
/// completed handler even if the Redis dedup marker was lost.
pub async fn process_webhook_job(
    state: &AppState,
    envelope: &WebhookJobEnvelope,
) -> Result<(), AppError> {
    let action = envelope.action.as_deref().unwrap_or("");

    if let Some(delivery_id) = envelope.delivery_id.as_deref() {
        match idempotency::claim_delivery(
            state,
            delivery_id,
            &envelope.event,
            envelope.action.as_deref(),
            &envelope.correlation_id,
        )
        .await
        {
            Ok(DeliveryClaim::Duplicate) => {
                info!(delivery_id, event = %envelope.event, action, "duplicate GitHub delivery skipped");
                return Ok(());
            }
            Ok(DeliveryClaim::Proceed) => {}
            Err(guard_error) => {
                // Fail open: Redis-level dedup still protects against most
                // duplicate execution; don't let a DB hiccup drop webhooks.
                error!(%guard_error, delivery_id, "idempotency ledger unavailable; proceeding without DB guard");
            }
        }
    }

    info!(
        delivery_id = ?envelope.delivery_id,
        event = %envelope.event,
        action,
        attempt = envelope.attempts,
        correlation_id = %envelope.correlation_id,
        "processing GitHub webhook"
    );

    let result = dispatch(state, envelope).await;

    if let Some(delivery_id) = envelope.delivery_id.as_deref() {
        let ledger_result = match &result {
            Ok(()) => idempotency::mark_completed(state, delivery_id).await,
            Err(handler_error) => idempotency::mark_failed(state, delivery_id, handler_error).await,
        };
        if let Err(ledger_error) = ledger_result {
            error!(%ledger_error, delivery_id, "failed to update webhook idempotency ledger");
        }
    }

    result
}

async fn dispatch(state: &AppState, envelope: &WebhookJobEnvelope) -> Result<(), AppError> {
    let action = envelope.action.as_deref().unwrap_or("");
    let payload = &envelope.payload;

    match envelope.event.as_str() {
        "issues" => match action {
            "opened" | "labeled" => handle_issue_labeled(state, payload).await?,
            "assigned" => handle_issue_assigned(state, payload).await?,
            "unassigned" => handle_issue_unassigned(state, payload).await?,
            "closed" => handle_issue_closed(state, payload).await?,
            "deleted" => handle_issue_deleted(state, payload).await?,
            _ => {
                info!(event = %envelope.event, action, "issues action ignored");
            }
        },
        "issue_comment" if action == "created" => {
            handle_issue_comment_created(state, payload).await?;
        }
        "pull_request" => {
            if action == "closed" {
                let merged = payload
                    .get("pull_request")
                    .and_then(|pr| pr.get("merged"))
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                if merged {
                    handle_pr_merged(state, payload).await?;
                }
            }
        }
        "installation" => {
            handle_installation(state, payload, &envelope.action).await?;
        }
        "installation_repositories" => {
            handle_installation_repositories(state, payload, &envelope.action).await?;
        }
        "label" => {
            let label = payload
                .get("label")
                .and_then(|value| value.get("name"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or("unknown");
            info!(
                action,
                label,
                "repository label definition changed; apply the label to an issue to trigger a bounty"
            );
        }
        _ => {
            info!(event = %envelope.event, action, "unsupported webhook event ignored");
        }
    }
    Ok(())
}

async fn handle_installation(
    state: &AppState,
    payload: &serde_json::Value,
    action: &Option<String>,
) -> Result<(), AppError> {
    let action = action.as_deref().unwrap_or("");

    if action != "created" && action != "deleted" {
        return Ok(());
    }

    let installation: GithubInstallation = serde_json::from_value(
        payload
            .get("installation")
            .cloned()
            .ok_or_else(|| AppError::bad_request("Missing installation data"))?,
    )
    .map_err(|e| AppError::bad_request(format!("Failed to parse installation: {}", e)))?;

    let sender: GithubSender = serde_json::from_value(
        payload
            .get("sender")
            .cloned()
            .ok_or_else(|| AppError::bad_request("Missing sender data"))?,
    )
    .map_err(|e| AppError::bad_request(format!("Failed to parse sender: {}", e)))?;

    if action == "created" {
        let repositories: Vec<GithubRepository> = serde_json::from_value(
            payload
                .get("repositories")
                .cloned()
                .unwrap_or(serde_json::Value::Array(vec![])),
        )
        .unwrap_or_default();

        for repo in repositories {
            // Filter out forks and private repositories
            if repo.fork || repo.private {
                info!(full_name = %repo.full_name, "skipping repo (fork/private)");
                continue;
            }

            if let Err(e) = upsert_installation_repo(
                state,
                repo.id,
                &repo.full_name,
                installation.account.id,
                &installation.account.login,
                &installation.account.account_type,
                repo.fork,
                repo.private,
                sender.id,
                installation.id,
            )
            .await
            {
                error!(
                    err = %e,
                    repo = %repo.full_name,
                    "failed to save repo on installation"
                );
            } else {
                info!(
                    repo = %repo.full_name,
                    installer = sender.id,
                    installation_id = installation.id,
                    "app installed on repo"
                );
            }
        }
    } else if action == "deleted" {
        // Delete all repos for this installation
        if let Err(e) = delete_repos_by_installation_id(state, installation.id).await {
            error!(
                err = %e,
                installation_id = installation.id,
                "failed to cleanup repos for uninstalled installation"
            );
        } else {
            info!(
                installation_id = installation.id,
                "cleared all repos for installation"
            );
        }
    }

    Ok(())
}

async fn handle_installation_repositories(
    state: &AppState,
    payload: &serde_json::Value,
    action: &Option<String>,
) -> Result<(), AppError> {
    let action = action.as_deref().unwrap_or("");

    if action != "added" && action != "removed" {
        return Ok(());
    }

    let installation: GithubInstallation = serde_json::from_value(
        payload
            .get("installation")
            .cloned()
            .ok_or_else(|| AppError::bad_request("Missing installation data"))?,
    )
    .map_err(|e| AppError::bad_request(format!("Failed to parse installation: {}", e)))?;

    let sender: GithubSender = serde_json::from_value(
        payload
            .get("sender")
            .cloned()
            .ok_or_else(|| AppError::bad_request("Missing sender data"))?,
    )
    .map_err(|e| AppError::bad_request(format!("Failed to parse sender: {}", e)))?;

    if action == "added" {
        let repositories_added: Vec<GithubRepository> = serde_json::from_value(
            payload
                .get("repositories_added")
                .cloned()
                .unwrap_or(serde_json::Value::Array(vec![])),
        )
        .unwrap_or_default();

        for repo in repositories_added {
            // Filter out forks and private repositories
            if repo.fork || repo.private {
                info!(full_name = %repo.full_name, "skipping added repo (fork/private)");
                continue;
            }

            if let Err(e) = upsert_installation_repo(
                state,
                repo.id,
                &repo.full_name,
                installation.account.id,
                &installation.account.login,
                &installation.account.account_type,
                repo.fork,
                repo.private,
                sender.id,
                installation.id,
            )
            .await
            {
                error!(
                    err = %e,
                    repo = %repo.full_name,
                    "failed to add repo to installation"
                );
            } else {
                info!(
                    repo = %repo.full_name,
                    installer = sender.id,
                    "added repo to installation"
                );
            }
        }
    } else if action == "removed" {
        let repositories_removed: Vec<GithubRepository> = serde_json::from_value(
            payload
                .get("repositories_removed")
                .cloned()
                .unwrap_or(serde_json::Value::Array(vec![])),
        )
        .unwrap_or_default();

        for repo in repositories_removed {
            info!(full_name = %repo.full_name, repo_id = repo.id, "removed repo from installation");
            if let Err(e) = delete_repo_by_github_id(state, repo.id).await {
                error!(
                    err = %e,
                    repo = %repo.full_name,
                    "failed to remove repo from DB"
                );
            } else {
                info!(full_name = %repo.full_name, "removed repo from DB");
            }
        }
    }

    Ok(())
}
