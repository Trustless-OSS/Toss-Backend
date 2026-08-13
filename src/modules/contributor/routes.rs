use axum::{
    extract::State,
    routing::{get, post},
    Json, Router,
};

use crate::{
    error::AppError,
    middleware::auth::AuthedUser,
    modules::contributor::model::{ConnectWalletBody, ContributorMeResponse, OkResponse},
    modules::repo::repository::{
        get_contributor_by_github_id, list_assignments_for_contributor, upsert_contributor_wallet,
    },
    state::AppState,
};

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

    let assignments = list_assignments_for_contributor(&state, contributor.id).await?;

    let mut assignment_rows = Vec::with_capacity(assignments.len());
    for (assignment, issue) in assignments {
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
