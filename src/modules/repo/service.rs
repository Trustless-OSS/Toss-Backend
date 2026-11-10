use rust_decimal::Decimal;
use tracing::{error, info};
use uuid::Uuid;

use crate::{
    error::AppError,
    modules::{
        bounty::repository::list_issues_for_repo,
        github::{
            auth::{
                delete_github_installation, install_repo_webhook, list_installation_repos,
                remove_repo_from_installation,
            },
            labels::{delete_github_label, list_github_labels, GitHubLabel},
            repository::upsert_installation_repo,
        },
        repo::{
            datatypes::RewardQuery,
            model::{
                ConnectRepoInput, InstallationRepoSummary, InstallationReposList, OkResponse,
                RepoAccessInput, RepoDetails, RepoResponse, SyncInstallationInput,
                SyncInstallationResult, UpdateRewardsInput,
            },
            repository::{
                count_repos_for_installation, delete_repo_cascade, delete_repo_reward,
                get_repo_by_id, get_repo_rewards, invalidate_repo_cache, is_maintainer,
                list_repos_for_user, upsert_repo_rewards, upsert_repo_with_maintainer,
            },
        },
    },
    shared::{
        models::{Repo, Reward},
        pagination::{PaginatedResponse, PaginationQuery},
    },
    state::AppState,
};

pub(crate) async fn list_repos(
    state: &AppState,
    github_id: i64,
    github_username: Option<&str>,
    limit: i64,
    offset: i64,
) -> Result<PaginatedResponse<Repo>, AppError> {
    let (data, total_count) =
        list_repos_for_user(state, github_id, github_username, limit, offset).await?;

    Ok(PaginatedResponse {
        data,
        total_count,
        limit,
        offset,
    })
}

pub(crate) async fn connect_repo(
    state: &AppState,
    input: ConnectRepoInput,
) -> Result<RepoResponse, AppError> {
    let (login, full_name) = crate::modules::github::authorization::verify_connect(
        &input.gh_token,
        input.actor_github_id,
        input.github_repo_id,
        &input.full_name,
    )
    .await?;
    install_repo_webhook(state, &full_name, &input.gh_token, &input.webhook_url).await?;

    let repo = upsert_repo_with_maintainer(
        state,
        input.github_repo_id,
        &full_name,
        None,
        input.actor_github_id,
        &login,
    )
    .await?;

    invalidate_repo_cache(state, repo.id, Some(repo.github_repo_id)).await;

    Ok(RepoResponse { repo })
}

pub(crate) async fn list_installation_repos_for_sync(
    state: &AppState,
    installation_id: i64,
    github_id: i64,
) -> Result<InstallationReposList, AppError> {
    if installation_id <= 0 {
        return Err(AppError::bad_request("Invalid installation ID"));
    }
    let repos = list_installation_repos(state, installation_id).await?;
    let eligible: Vec<_> = repos
        .into_iter()
        .filter(|repo| !repo.fork && !repo.private)
        .collect();
    let names: Vec<_> = eligible.iter().map(|repo| repo.full_name.clone()).collect();
    let (_, allowed) = crate::modules::github::authorization::installation_repo_admins(
        state,
        installation_id,
        github_id,
        &names,
    )
    .await?;
    let repositories = eligible
        .into_iter()
        .filter(|repo| allowed.contains(&repo.full_name))
        .map(|repo| InstallationRepoSummary {
            github_repo_id: repo.id,
            full_name: repo.full_name,
        })
        .collect();

    Ok(InstallationReposList { repositories })
}

pub(crate) async fn sync_installation(
    state: &AppState,
    input: SyncInstallationInput,
) -> Result<SyncInstallationResult, AppError> {
    if input.installation_id <= 0 {
        return Err(AppError::bad_request("Invalid installation ID"));
    }
    let repos = list_installation_repos(state, input.installation_id).await?;
    let mut synced = 0;

    let filter_ids: Option<std::collections::HashSet<i64>> =
        if let Some(ids) = input.github_repo_ids.as_ref().filter(|ids| !ids.is_empty()) {
            Some(ids.iter().copied().collect())
        } else {
            input
                .github_repo_id
                .map(|id| std::collections::HashSet::from([id]))
        };

    let mut selected = Vec::new();
    for repo in repos {
        if repo.fork || repo.private {
            continue;
        }

        if let Some(ref ids) = filter_ids {
            if !ids.contains(&repo.id) {
                continue;
            }
        }

        selected.push(repo);
    }

    if filter_ids.is_some() && selected.is_empty() {
        return Err(AppError::not_found(
            "Requested public installation repositories not found",
        ));
    }
    let names: Vec<_> = selected.iter().map(|repo| repo.full_name.clone()).collect();
    let (login, allowed) = crate::modules::github::authorization::installation_repo_admins(
        state,
        input.installation_id,
        input.installer_github_id,
        &names,
    )
    .await?;
    if filter_ids.is_some()
        && selected
            .iter()
            .any(|repo| !allowed.contains(&repo.full_name))
    {
        return Err(AppError::forbidden(
            "A selected repository is not administered by the caller",
        ));
    }
    // Complete authorization for the whole requested batch before any database write.
    let mut writes = tokio::task::JoinSet::new();
    for repo in selected
        .into_iter()
        .filter(|repo| allowed.contains(&repo.full_name))
    {
        let state = state.clone();
        let login = login.clone();
        let installer_id = input.installer_github_id;
        let installation_id = input.installation_id;
        writes.spawn(async move {
            upsert_installation_repo(
                &state,
                repo.id,
                &repo.full_name,
                installer_id,
                &login,
                installation_id,
            )
            .await
        });
        if writes.len() >= 8 {
            writes
                .join_next()
                .await
                .ok_or_else(|| AppError::internal("Repository sync task disappeared"))?
                .map_err(|_| AppError::internal("Repository sync task failed"))??;
            synced += 1;
        }
    }
    while let Some(result) = writes.join_next().await {
        result.map_err(|_| AppError::internal("Repository sync task failed"))??;
        synced += 1;
    }

    if filter_ids.is_some() && synced == 0 {
        return Err(AppError::not_found(
            "Requested installation repositories were not found or are private/forked",
        ));
    }

    Ok(SyncInstallationResult { synced })
}

pub(crate) async fn list_issues(
    state: &AppState,
    repo_id: Uuid,
    pagination: PaginationQuery,
) -> Result<PaginatedResponse<serde_json::Value>, AppError> {
    let (limit, offset) = pagination.resolve();
    let (data, total_count) = list_issues_for_repo(state, repo_id, limit, offset).await?;

    Ok(PaginatedResponse {
        data,
        total_count,
        limit,
        offset,
    })
}

pub(crate) async fn repo_details(
    state: &AppState,
    input: RepoAccessInput,
) -> Result<RepoDetails, AppError> {
    let repo = get_repo_by_id(state, input.repo_id)
        .await?
        .ok_or_else(|| AppError::not_found("Repo not found"))?;

    let is_maintainer = is_maintainer(state, input.github_id, repo.id).await?;
    Ok(RepoDetails::new(repo, is_maintainer))
}

pub(crate) async fn update_rewards(
    state: &AppState,
    input: UpdateRewardsInput,
) -> Result<RepoResponse, AppError> {
    if !is_maintainer(state, input.maintainer_github_id, input.repo_id).await? {
        return Err(AppError::forbidden(
            "Forbidden: Only maintainers can update reward levels",
        ));
    }

    let repo = get_repo_by_id(state, input.repo_id)
        .await?
        .ok_or_else(|| AppError::not_found("Repo not found"))?;
    require_live_maintainer(state, &repo, input.maintainer_github_id).await?;

    crate::shared::money::validate_reward(input.amount, false)?;

    if input.label.is_empty() {
        return Err(AppError::bad_request("Reward Label must be non-negative"));
    }

    let repo = upsert_repo_rewards(state, input.repo_id, input.label, input.amount).await?;

    Ok(RepoResponse { repo })
}

pub(crate) async fn get_repo_rewards_by_repo_id(
    state: &AppState,
    repo_id: Uuid,
) -> Result<Vec<Reward>, AppError> {
    get_repo_rewards(state, RewardQuery::ByRepo(repo_id)).await
}

pub(crate) async fn delete_repo(
    state: &AppState,
    input: RepoAccessInput,
) -> Result<OkResponse, AppError> {
    if !is_maintainer(state, input.github_id, input.repo_id).await? {
        return Err(AppError::forbidden(
            "Forbidden: Only maintainers can delete a repository",
        ));
    }

    let repo = get_repo_by_id(state, input.repo_id)
        .await?
        .ok_or_else(|| AppError::not_found("Repo not found"))?;

    require_live_maintainer(state, &repo, input.github_id).await?;

    let balance = repo.escrow_balance.unwrap_or(Decimal::ZERO);
    if balance > Decimal::ZERO {
        return Err(AppError::bad_request(
            "Repo still has escrow funds. Withdraw all funds before deleting.",
        ));
    }

    if let Some(installation_id) = repo.github_install_id {
        match count_repos_for_installation(state, installation_id, input.repo_id).await {
            Ok(0) => {
                if let Err(error) = delete_github_installation(state, installation_id).await {
                    error!(%error, "GitHub App uninstall failed (continuing with DB cleanup)");
                } else {
                    info!(
                        installation_id,
                        repo = %repo.full_name,
                        "uninstalled GitHub App (last repo)"
                    );
                }
            }
            Ok(_) => {
                if let Err(error) =
                    remove_repo_from_installation(state, installation_id, repo.github_repo_id).await
                {
                    error!(
                        %error,
                        repo = %repo.full_name,
                        "could not remove repo from installation"
                    );
                } else {
                    info!(
                        repo = %repo.full_name,
                        installation_id,
                        "removed repository from installation"
                    );
                }
            }
            Err(error) => {
                error!(%error, "GitHub App management failed (continuing with DB cleanup)");
            }
        }
    }

    delete_repo_cascade(state, input.repo_id).await?;
    invalidate_repo_cache(state, input.repo_id, Some(repo.github_repo_id)).await;
    info!(repo = %repo.full_name, repo_id = %input.repo_id, "repo deleted");

    Ok(OkResponse { ok: true })
}

pub(crate) async fn get_github_labels(
    state: &AppState,
    repo_id: Uuid,
    github_id: i64,
) -> Result<Vec<GitHubLabel>, AppError> {
    let repo = get_repo_by_id(state, repo_id)
        .await?
        .ok_or_else(|| AppError::not_found("Repo not found"))?;
    if !is_maintainer(state, github_id, repo_id).await? {
        return Err(AppError::forbidden(
            "Forbidden: Only maintainers can view GitHub labels",
        ));
    }
    list_github_labels(state, repo.github_repo_id, &repo.full_name).await
}

pub(crate) async fn delete_reward(
    state: &AppState,
    repo_id: Uuid,
    maintainer_github_id: i64,
    label: String,
    also_delete_github_label: bool,
) -> Result<OkResponse, AppError> {
    if !is_maintainer(state, maintainer_github_id, repo_id).await? {
        return Err(AppError::forbidden(
            "Forbidden: Only maintainers can delete reward levels",
        ));
    }

    let repo = get_repo_by_id(state, repo_id)
        .await?
        .ok_or_else(|| AppError::not_found("Repo not found"))?;
    require_live_maintainer(state, &repo, maintainer_github_id).await?;

    delete_repo_reward(state, repo_id, &label).await?;
    invalidate_repo_cache(state, repo_id, None).await;

    if also_delete_github_label {
        if let Some(repo) = get_repo_by_id(state, repo_id).await? {
            if let Err(e) =
                delete_github_label(state, repo.github_repo_id, &repo.full_name, &label).await
            {
                tracing::warn!(
                    err = %e,
                    label = %label,
                    "could not delete GitHub label (continuing)"
                );
            }
        }
    }

    Ok(OkResponse { ok: true })
}

async fn require_live_maintainer(
    state: &AppState,
    repo: &crate::shared::models::Repo,
    user_id: i64,
) -> Result<(), AppError> {
    crate::modules::github::authorization::require_current_repo_maintainer(state, repo, user_id)
        .await
        .map_err(|error| match error {
            AppError::Forbidden { .. } => {
                AppError::forbidden("Current GitHub maintainer access is required")
            }
            other => other,
        })
}
