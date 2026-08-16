use axum::{
    extract::{Path, Query, State},
    http::HeaderMap,
    Json,
};
use rust_decimal::Decimal;
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    error::AppError,
    middleware::auth::AuthedUser,
    modules::repo::{
        model::{
            ConnectRepoInput, OkResponse, RepoAccessInput, RepoDetails, RepoResponse,
            SyncInstallationInput, SyncInstallationResult, UpdateRewardsInput,
        },
        service,
    },
    shared::{
        models::Repo,
        pagination::{PaginatedQuery, PaginatedResponse, PaginationQuery},
    },
    state::AppState,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConnectRepoBody {
    github_repo_id: i64,
    full_name: String,
    owner_github_id: i64,
    owner_username: String,
    gh_token: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct UpdateRewardsBody {
    reward_low: Decimal,
    reward_medium: Decimal,
    reward_high: Decimal,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SyncInstallationBody {
    #[serde(alias = "installation_id")]
    installation_id: i64,
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

pub(crate) async fn list_repos(
    State(state): State<AppState>,
    user: AuthedUser,
    Query(pagination): PaginatedQuery,
) -> Result<Json<PaginatedResponse<Repo>>, AppError> {
    let (limit, offset) = pagination.resolve();
    let response = service::list_repos(
        &state,
        user.github_id,
        user.github_username.as_deref(),
        limit,
        offset,
    )
    .await?;

    Ok(Json(response))
}

pub(crate) async fn connect_repo(
    State(state): State<AppState>,
    headers: HeaderMap,
    _user: AuthedUser,
    Json(body): Json<ConnectRepoBody>,
) -> Result<Json<RepoResponse>, AppError> {
    let input = ConnectRepoInput {
        github_repo_id: body.github_repo_id,
        full_name: body.full_name,
        owner_github_id: body.owner_github_id,
        owner_username: body.owner_username,
        gh_token: body.gh_token,
        webhook_url: resolve_webhook_url(&state, &headers),
    };
    let response = service::connect_repo(&state, input).await?;
    Ok(Json(response))
}

pub(crate) async fn sync_installation(
    State(state): State<AppState>,
    user: AuthedUser,
    Json(body): Json<SyncInstallationBody>,
) -> Result<Json<SyncInstallationResult>, AppError> {
    let input = SyncInstallationInput {
        installation_id: body.installation_id,
        installer_github_id: user.github_id,
    };
    let response = service::sync_installation(&state, input).await?;
    Ok(Json(response))
}

pub(crate) async fn list_issues(
    State(state): State<AppState>,
    Path(repo_id): Path<Uuid>,
    Query(pagination): Query<PaginationQuery>,
) -> Result<Json<PaginatedResponse<serde_json::Value>>, AppError> {
    let response = service::list_issues(&state, repo_id, pagination).await?;
    Ok(Json(response))
}

pub(crate) async fn repo_details(
    State(state): State<AppState>,
    user: AuthedUser,
    Path(repo_id): Path<Uuid>,
) -> Result<Json<RepoDetails>, AppError> {
    let response = service::repo_details(
        &state,
        RepoAccessInput {
            repo_id,
            github_id: user.github_id,
        },
    )
    .await?;
    Ok(Json(response))
}

pub(crate) async fn update_rewards(
    State(state): State<AppState>,
    user: AuthedUser,
    Path(repo_id): Path<Uuid>,
    Json(body): Json<UpdateRewardsBody>,
) -> Result<Json<RepoResponse>, AppError> {
    let input = UpdateRewardsInput {
        repo_id,
        maintainer_github_id: user.github_id,
        reward_low: body.reward_low,
        reward_medium: body.reward_medium,
        reward_high: body.reward_high,
    };
    let response = service::update_rewards(&state, input).await?;
    Ok(Json(response))
}

pub(crate) async fn delete_repo(
    State(state): State<AppState>,
    user: AuthedUser,
    Path(repo_id): Path<Uuid>,
) -> Result<Json<OkResponse>, AppError> {
    let response = service::delete_repo(
        &state,
        RepoAccessInput {
            repo_id,
            github_id: user.github_id,
        },
    )
    .await?;
    Ok(Json(response))
}

#[cfg(test)]
mod tests {
    use super::SyncInstallationBody;

    #[test]
    fn sync_installation_body_accepts_camel_and_snake_case() {
        let camel: SyncInstallationBody =
            serde_json::from_str(r#"{"installationId": 153860735}"#).unwrap();
        let snake: SyncInstallationBody =
            serde_json::from_str(r#"{"installation_id": 153860735}"#).unwrap();
        assert_eq!(camel.installation_id, 153860735);
        assert_eq!(snake.installation_id, 153860735);
    }
}
