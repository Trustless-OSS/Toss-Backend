use axum::{
    extract::{Path, Query, State},
    http::HeaderMap,
    routing::{delete, get, post, put},
    Json, Router,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use tracing::{error, info};
use uuid::Uuid;

use crate::{
    error::{require_db, AppError},
    middleware::auth::AuthedUser,
    modules::github::auth::{
        delete_github_installation, install_repo_webhook, list_installation_repos,
        remove_repo_from_installation,
    },
    modules::repo::repository::{
        count_repos_for_installation, delete_repo_cascade, get_repo_by_id, invalidate_repo_cache,
        is_maintainer, list_issues_for_repo, list_repos_for_user, update_repo_rewards,
        upsert_installation_repo, upsert_repo,
    },
    shared::models::Repo,
    shared::pagination::{PaginatedQuery, PaginatedResponse, PaginationQuery},
    state::AppState,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConnectRepoBody {
    github_repo_id: i64,
    full_name: String,
    owner_github_id: i64,
    owner_username: String,
    gh_token: String,
}

#[derive(Debug, Deserialize)]
struct UpdateRewardsBody {
    reward_low: Decimal,
    reward_medium: Decimal,
    reward_high: Decimal,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SyncInstallationBody {
    installation_id: i64,
}

#[derive(Debug, Serialize)]
struct ConnectRepoResponse {
    repo: Repo,
}

#[derive(Debug, Serialize)]
struct UpdateRewardsResponse {
    repo: Repo,
}

#[derive(Debug, Serialize)]
struct SyncInstallationResponse {
    synced: usize,
}

#[derive(Debug, Serialize)]
struct OkResponse {
    ok: bool,
}

fn resolve_webhook_url(state: &AppState, headers: &HeaderMap) -> String {
    if let Some(url) = state.config.webhook_url.as_deref() {
        return url.to_string();
    }

    let protocol = headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("http");
    let host = headers.get("host").and_then(|value| value.to_str().ok());

    host.map(|host| format!("{protocol}://{host}/api/webhooks/github"))
        .unwrap_or_else(|| "https://smee.io/trustless-oss-dev-webhook".to_string())
}

//  List all repos on Dashbord
async fn list_repos(
    State(state): State<AppState>,
    user: AuthedUser,
    Query(pagination): PaginatedQuery,
) -> Result<Json<PaginatedResponse<Repo>>, AppError> {
    let (limit, offset) = pagination.resolve();
    let (data, total_count) = list_repos_for_user(
        &state,
        user.github_id,
        user.github_username.as_deref(),
        limit,
        offset,
    )
    .await?;

    Ok(Json(PaginatedResponse {
        data,
        total_count,
        limit,
        offset,
    }))
}

async fn connect_repo(
    State(state): State<AppState>,
    headers: HeaderMap,
    _user: AuthedUser,
    Json(body): Json<ConnectRepoBody>,
) -> Result<Json<ConnectRepoResponse>, AppError> {
    let webhook_url = resolve_webhook_url(&state, &headers);

    if let Err(error) =
        install_repo_webhook(&state, &body.full_name, &body.gh_token, &webhook_url).await
    {
        error!(err = %error, repo = %body.full_name, "exception while installing webhook");
    }

    let pool = require_db(&state.db)?;
    let repo = upsert_repo(
        pool,
        body.github_repo_id,
        &body.full_name,
        body.owner_github_id,
        &body.owner_username,
    )
    .await?;

    invalidate_repo_cache(&state, repo.id, Some(repo.github_repo_id)).await;

    Ok(Json(ConnectRepoResponse { repo }))
}

async fn sync_installation(
    State(state): State<AppState>,
    user: AuthedUser,
    Json(body): Json<SyncInstallationBody>,
) -> Result<Json<SyncInstallationResponse>, AppError> {
    let repos = list_installation_repos(&state, body.installation_id).await?;
    let pool = require_db(&state.db)?;
    let mut synced = 0;

    for repo in repos {
        if repo.fork || repo.private {
            continue;
        }

        upsert_installation_repo(
            pool,
            repo.id,
            &repo.full_name,
            repo.owner.id,
            &repo.owner.login,
            &repo.owner.account_type,
            repo.fork,
            repo.private,
            user.github_id,
            body.installation_id,
        )
        .await?;

        synced += 1;
    }

    Ok(Json(SyncInstallationResponse { synced }))
}

async fn list_issues(
    State(state): State<AppState>,
    Path(repo_id): Path<Uuid>,
    Query(pagination): Query<PaginationQuery>,
) -> Result<Json<PaginatedResponse<serde_json::Value>>, AppError> {
    let (limit, offset) = pagination.resolve();
    let (data, total_count) = list_issues_for_repo(&state, repo_id, limit, offset).await?;

    Ok(Json(PaginatedResponse {
        data,
        total_count,
        limit,
        offset,
    }))
}

async fn update_rewards(
    State(state): State<AppState>,
    user: AuthedUser,
    Path(repo_id): Path<Uuid>,
    Json(body): Json<UpdateRewardsBody>,
) -> Result<Json<UpdateRewardsResponse>, AppError> {
    if !is_maintainer(&state, user.github_id, repo_id).await? {
        return Err(AppError::forbidden(
            "Forbidden: Only maintainers can update reward levels",
        ));
    }

    if body.reward_low < Decimal::ZERO
        || body.reward_medium < Decimal::ZERO
        || body.reward_high < Decimal::ZERO
    {
        return Err(AppError::bad_request("Reward amounts must be non-negative"));
    }

    let repo = update_repo_rewards(
        &state,
        repo_id,
        body.reward_low,
        body.reward_medium,
        body.reward_high,
    )
    .await?;

    Ok(Json(UpdateRewardsResponse { repo }))
}

async fn delete_repo(
    State(state): State<AppState>,
    user: AuthedUser,
    Path(repo_id): Path<Uuid>,
) -> Result<Json<OkResponse>, AppError> {
    if !is_maintainer(&state, user.github_id, repo_id).await? {
        return Err(AppError::forbidden(
            "Forbidden: Only maintainers can delete a repository",
        ));
    }

    let repo = get_repo_by_id(&state, repo_id)
        .await?
        .ok_or_else(|| AppError::not_found("Repo not found"))?;

    if repo.escrow_balance > Decimal::ZERO {
        return Err(AppError::bad_request(
            "Repo still has escrow funds. Withdraw all funds before deleting.",
        ));
    }

    if let Some(installation_id) = repo.github_installation_id {
        match count_repos_for_installation(&state, installation_id, repo_id).await {
            Ok(other_repos) if other_repos == 0 => {
                if let Err(error) = delete_github_installation(&state, installation_id).await {
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
                    remove_repo_from_installation(&state, installation_id, repo.github_repo_id)
                        .await
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

    delete_repo_cascade(&state, repo_id).await?;
    invalidate_repo_cache(&state, repo_id, Some(repo.github_repo_id)).await;
    info!(repo = %repo.full_name, %repo_id, "repo deleted");

    Ok(Json(OkResponse { ok: true }))
}

//  Repo routes
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/repos", get(list_repos))
        .route("/api/repos/connect", post(connect_repo))
        .route("/api/repos/sync-installation", post(sync_installation))
        .route("/api/repos/{repoId}/issues", get(list_issues))
        .route("/api/repos/{repoId}/rewards", put(update_rewards))
        .route("/api/repos/{repoId}", delete(delete_repo))
}
