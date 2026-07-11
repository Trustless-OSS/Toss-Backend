use rust_decimal::Decimal;
use tracing::{error, info};
use uuid::Uuid;

use crate::{
    error::AppError,
    modules::{
        github::auth::{
            delete_github_installation, install_repo_webhook, list_installation_repos,
            remove_repo_from_installation,
        },
        repo::{
            model::{
                ConnectRepoInput, OkResponse, RepoAccessInput, RepoDetails, RepoResponse,
                SyncInstallationInput, SyncInstallationResult, UpdateRewardsInput,
            },
            repository::{
                count_repos_for_installation, delete_repo_cascade, get_repo_by_id,
                invalidate_repo_cache, is_maintainer, list_issues_for_repo, list_repos_for_user,
                update_repo_rewards, upsert_installation_repo, upsert_repo,
            },
        },
    },
    shared::{
        models::Repo,
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
    if let Err(error) =
        install_repo_webhook(state, &input.full_name, &input.gh_token, &input.webhook_url).await
    {
        error!(err = %error, repo = %input.full_name, "exception while installing webhook");
    }

    let repo = upsert_repo(
        state,
        input.github_repo_id,
        &input.full_name,
        input.owner_github_id,
        &input.owner_username,
    )
    .await?;

    invalidate_repo_cache(state, repo.id, Some(repo.github_repo_id)).await;

    Ok(RepoResponse { repo })
}

pub(crate) async fn sync_installation(
    state: &AppState,
    input: SyncInstallationInput,
) -> Result<SyncInstallationResult, AppError> {
    let repos = list_installation_repos(state, input.installation_id).await?;
    let mut synced = 0;

    for repo in repos {
        if repo.fork || repo.private {
            continue;
        }

        upsert_installation_repo(
            state,
            repo.id,
            &repo.full_name,
            repo.owner.id,
            &repo.owner.login,
            &repo.owner.account_type,
            repo.fork,
            repo.private,
            input.installer_github_id,
            input.installation_id,
        )
        .await?;

        synced += 1;
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

    if input.reward_low < Decimal::ZERO
        || input.reward_medium < Decimal::ZERO
        || input.reward_high < Decimal::ZERO
    {
        return Err(AppError::bad_request("Reward amounts must be non-negative"));
    }

    let repo = update_repo_rewards(
        state,
        input.repo_id,
        input.reward_low,
        input.reward_medium,
        input.reward_high,
    )
    .await?;

    Ok(RepoResponse { repo })
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

    if repo.escrow_balance > Decimal::ZERO {
        return Err(AppError::bad_request(
            "Repo still has escrow funds. Withdraw all funds before deleting.",
        ));
    }

    if let Some(installation_id) = repo.github_installation_id {
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
                        "could not remove repo from installation (might be 'All repositories' selection)"
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
