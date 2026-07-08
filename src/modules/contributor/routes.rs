use axum::{
    extract::State,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};

use crate::{
    error::{require_db, AppError},
    middleware::auth::AuthedUser,
    modules::repo::repository::{get_contributor_by_github_id, upsert_contributor_wallet},
    shared::models::{Assignment, Issue},
    state::AppState,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConnectWalletBody {
    wallet: String,
    payout_chain: Option<String>,
    payout_address: Option<String>,
}

#[derive(Debug, Serialize)]
struct OkResponse {
    ok: bool,
}

#[derive(Debug, Serialize)]
struct ContributorMeResponse {
    contributor: Option<serde_json::Value>,
}

async fn connect_wallet(
    State(state): State<AppState>,
    user: AuthedUser,
    Json(body): Json<ConnectWalletBody>,
) -> Result<Json<OkResponse>, AppError> {
    let payout_chain = body
        .payout_chain
        .as_deref()
        .unwrap_or("stellar")
        .to_string();
    let payout_address = body
        .payout_address
        .as_deref()
        .unwrap_or(&body.wallet)
        .to_string();

    upsert_contributor_wallet(
        &state,
        user.github_id,
        user.github_username.as_deref().unwrap_or(""),
        &payout_chain,
        &payout_address,
    )
    .await?;

    Ok(Json(OkResponse { ok: true }))
}

async fn get_contributor_me(
    State(state): State<AppState>,
    user: AuthedUser,
) -> Result<Json<ContributorMeResponse>, AppError> {
    let contributor = get_contributor_by_github_id(&state, user.github_id).await?;

    let Some(contributor) = contributor else {
        return Ok(Json(ContributorMeResponse { contributor: None }));
    };

    let pool = require_db(&state.db)?;
    let assignments =
        sqlx::query_as::<_, Assignment>("SELECT * FROM assignments WHERE contributor_id = $1")
            .bind(contributor.id)
            .fetch_all(pool)
            .await
            .map_err(|error| AppError::database(error.to_string()))?;

    let mut assignment_rows = Vec::with_capacity(assignments.len());
    for assignment in assignments {
        let issue = sqlx::query_as::<_, Issue>("SELECT * FROM issues WHERE id = $1")
            .bind(assignment.issue_id)
            .fetch_optional(pool)
            .await
            .map_err(|error| AppError::database(error.to_string()))?;

        assignment_rows.push(serde_json::json!({
            "id": assignment.id,
            "issue_id": assignment.issue_id,
            "contributor_id": assignment.contributor_id,
            "assigned_at": assignment.assigned_at,
            "pr_number": assignment.pr_number,
            "pr_merged_at": assignment.pr_merged_at,
            "payout_status": assignment.payout_status,
            "completion_percentage": assignment.completion_percentage,
            "issues": issue,
        }));
    }

    let contributor_json = serde_json::json!({
        "id": contributor.id,
        "github_user_id": contributor.github_user_id,
        "github_username": contributor.github_username,
        "stellar_wallet": contributor.stellar_wallet,
        "payout_chain": contributor.payout_chain,
        "payout_address": contributor.payout_address,
        "created_at": contributor.created_at,
        "assignments": assignment_rows,
    });

    Ok(Json(ContributorMeResponse {
        contributor: Some(contributor_json),
    }))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/wallet/connect", post(connect_wallet))
        .route("/api/contributor/me", get(get_contributor_me))
}
