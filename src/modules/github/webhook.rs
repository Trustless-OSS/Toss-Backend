use serde::Deserialize;
use tracing::{error, info};

use crate::{error::AppError, infra::queue::WebhookJobData, state::AppState};

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
    match job.event.as_str() {
        "installation" => {
            handle_installation(state, &job.payload, &job.action).await?;
        }
        "installation_repositories" => {
            handle_installation_repositories(state, &job.payload, &job.action).await?;
        }
        _ => {
            info!(event = %job.event, "webhook event not implemented");
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

            if let Err(e) = upsert_repo_to_db(
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
        if let Err(e) = delete_repos_for_installation(state, installation.id).await {
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

            if let Err(e) = upsert_repo_to_db(
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
            if let Err(e) = delete_repo_from_db(state, repo.id).await {
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

async fn upsert_repo_to_db(
    state: &AppState,
    github_repo_id: i64,
    full_name: &str,
    owner_github_id: i64,
    owner_username: &str,
    owner_type: &str,
    is_fork: bool,
    is_private: bool,
    installer_github_id: i64,
    github_installation_id: i64,
) -> Result<(), AppError> {
    let pool = state
        .db
        .as_ref()
        .ok_or_else(|| AppError::internal("Database not available"))?;

    sqlx::query(
        "INSERT INTO repos (github_repo_id, full_name, owner_github_id, owner_username, owner_type, is_fork, is_private, reward_low, reward_medium, reward_high, installer_github_id, github_installation_id)
         VALUES ($1, $2, $3, $4, $5, $6, $7, 1, 2, 3, $8, $9)
         ON CONFLICT (github_repo_id) DO UPDATE SET
           full_name = EXCLUDED.full_name,
           owner_github_id = EXCLUDED.owner_github_id,
           owner_username = EXCLUDED.owner_username,
           owner_type = EXCLUDED.owner_type,
           is_fork = EXCLUDED.is_fork,
           is_private = EXCLUDED.is_private,
           installer_github_id = EXCLUDED.installer_github_id,
           github_installation_id = EXCLUDED.github_installation_id"
    )
    .bind(github_repo_id)
    .bind(full_name)
    .bind(owner_github_id)
    .bind(owner_username)
    .bind(owner_type)
    .bind(is_fork)
    .bind(is_private)
    .bind(installer_github_id)
    .bind(github_installation_id)
    .execute(pool)
    .await
    .map_err(|e| AppError::database(format!("Failed to upsert repo: {}", e)))?;

    Ok(())
}

async fn delete_repos_for_installation(
    state: &AppState,
    installation_id: i64,
) -> Result<(), AppError> {
    let pool = state
        .db
        .as_ref()
        .ok_or_else(|| AppError::internal("Database not available"))?;

    sqlx::query("DELETE FROM repos WHERE github_installation_id = $1")
        .bind(installation_id)
        .execute(pool)
        .await
        .map_err(|e| AppError::database(format!("Failed to delete repos: {}", e)))?;

    Ok(())
}

async fn delete_repo_from_db(state: &AppState, github_repo_id: i64) -> Result<(), AppError> {
    let pool = state
        .db
        .as_ref()
        .ok_or_else(|| AppError::internal("Database not available"))?;

    sqlx::query("DELETE FROM repos WHERE github_repo_id = $1")
        .bind(github_repo_id)
        .execute(pool)
        .await
        .map_err(|e| AppError::database(format!("Failed to delete repo: {}", e)))?;

    Ok(())
}
