use serde::Deserialize;
use tracing::{error, info};

use crate::{
    error::AppError,
    infra::queue::WebhookJobData,
    modules::github::handlers::{
        handle_issue_assigned, handle_issue_closed, handle_issue_comment_created,
        handle_issue_deleted, handle_issue_labeled, handle_issue_unassigned, handle_pr_merged,
    },
    modules::github::repository::{
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

pub async fn process_webhook_job(state: &AppState, job: WebhookJobData) -> Result<(), AppError> {
    let action = job.action.as_deref().unwrap_or("");
    info!(
        event = %job.event,
        action,
        "processing GitHub webhook"
    );

    match job.event.as_str() {
        "issues" => match action {
            "opened" | "labeled" => handle_issue_labeled(state, &job.payload).await?,
            "assigned" => handle_issue_assigned(state, &job.payload).await?,
            "unassigned" => handle_issue_unassigned(state, &job.payload).await?,
            "closed" => handle_issue_closed(state, &job.payload).await?,
            "deleted" => handle_issue_deleted(state, &job.payload).await?,
            _ => {
                info!(event = %job.event, action, "issues action ignored");
            }
        },
        "issue_comment" if action == "created" => {
            handle_issue_comment_created(state, &job.payload).await?;
        }
        "pull_request" => {
            if action == "closed" {
                let merged = job
                    .payload
                    .get("pull_request")
                    .and_then(|pr| pr.get("merged"))
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                if merged {
                    handle_pr_merged(state, &job.payload).await?;
                }
            }
        }
        "installation" => {
            handle_installation(state, &job.payload, &job.action).await?;
        }
        "installation_repositories" => {
            handle_installation_repositories(state, &job.payload, &job.action).await?;
        }
        "label" => {
            let label = job
                .payload
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
            info!(event = %job.event, action, "unsupported webhook event ignored");
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
