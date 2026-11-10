use serde::Deserialize;
use tracing::info;

use crate::{
    error::AppError,
    infra::queue::WebhookJobData,
    modules::github::handlers::{
        handle_issue_assigned, handle_issue_closed, handle_issue_comment_created,
        handle_issue_deleted, handle_issue_labeled, handle_issue_unassigned, handle_pr_merged,
    },
    modules::github::repository::{
        detach_repo_from_installation, detach_repos_by_installation_id, sync_repo_full_name,
        upsert_installation_repo,
    },
    state::AppState,
};

#[derive(Debug, Deserialize)]
struct GithubInstallation {
    id: i64,
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
    if let Some(task_id) = job.payload["_recoveryTaskId"]
        .as_str()
        .and_then(|id| uuid::Uuid::parse_str(id).ok())
    {
        let mut db = crate::error::require_db(&state.db)?;
        let task = crate::shared::models::schema::FailedTask::filter_by_id(task_id)
            .first()
            .exec(&mut db)
            .await
            .map_err(crate::error::map_db_err)?;
        if task.is_none_or(|task| !crate::infra::recovery::accepts_recovered_webhook(&task.status))
        {
            return Ok(());
        }
    }
    let action = job.action.as_deref().unwrap_or("invailid");
    info!(
        event = %job.event,
        action,
        "processing GitHub webhook"
    );

    // Repo-scoped events carry repository.{id,full_name}; keep the stored name in
    // sync so renamed orgs don't break full_name-keyed comment/REST lookups.
    if matches!(
        job.event.as_str(),
        "issues" | "issue_comment" | "pull_request"
    ) {
        if let Some(repo) = job.payload.get("repository") {
            if let (Some(id), Some(full_name)) = (
                repo.get("id").and_then(serde_json::Value::as_i64),
                repo.get("full_name").and_then(serde_json::Value::as_str),
            ) {
                sync_repo_full_name(state, id, full_name).await?;
            }
        }
    }

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

        let synced = import_verified_repos(state, repositories, installation.id, sender.id).await?;
        info!(
            synced,
            installer = sender.id,
            installation_id = installation.id,
            "app installation repositories synced"
        );
    } else if action == "deleted" {
        detach_repos_by_installation_id(state, installation.id).await?;
        info!(
            installation_id = installation.id,
            "detached installation; repository and escrow records retained"
        );
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

        let synced =
            import_verified_repos(state, repositories_added, installation.id, sender.id).await?;
        info!(
            synced,
            installer = sender.id,
            installation_id = installation.id,
            "added installation repositories synced"
        );
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
            detach_repo_from_installation(state, repo.id, installation.id).await?;
            info!(full_name = %repo.full_name, "repo detached; escrow and bounties retained");
        }
    }

    Ok(())
}

async fn import_verified_repos(
    state: &AppState,
    repositories: Vec<GithubRepository>,
    installation_id: i64,
    sender_id: i64,
) -> Result<usize, AppError> {
    let eligible: Vec<_> = repositories
        .into_iter()
        .filter(|repo| !repo.fork && !repo.private)
        .collect();
    if eligible.is_empty() {
        return Ok(0);
    }
    let names: Vec<_> = eligible.iter().map(|repo| repo.full_name.clone()).collect();
    let (login, allowed) =
        super::authorization::installation_repo_admins(state, installation_id, sender_id, &names)
            .await?;
    let mut writes = tokio::task::JoinSet::new();
    let mut synced = 0;
    for repo in eligible
        .into_iter()
        .filter(|repo| allowed.contains(&repo.full_name))
    {
        let state = state.clone();
        let login = login.clone();
        writes.spawn(async move {
            upsert_installation_repo(
                &state,
                repo.id,
                &repo.full_name,
                sender_id,
                &login,
                installation_id,
            )
            .await
        });
        if writes.len() >= 8 {
            writes
                .join_next()
                .await
                .ok_or_else(|| AppError::internal("Installation import task disappeared"))?
                .map_err(|_| AppError::internal("Installation import task failed"))??;
            synced += 1;
        }
    }
    while let Some(result) = writes.join_next().await {
        result.map_err(|_| AppError::internal("Installation import task failed"))??;
        synced += 1;
    }
    Ok(synced)
}
