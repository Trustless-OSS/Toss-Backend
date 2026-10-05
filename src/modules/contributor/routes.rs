use axum::{
    extract::State,
    routing::{get, post},
    Json, Router,
};
use tracing::{info, warn};

use crate::{
    error::{AppError, ErrorResponse},
    infra::queue::BountyJobData,
    middleware::auth::AuthedUser,
    modules::contributor::{
        model::{ConnectWalletBody, ContributorMeResponse, OkResponse},
        repository::{
            get_profile_by_github_id, list_bounties_for_contributor, upsert_contributor_wallet,
        },
    },
    state::AppState,
};

#[utoipa::path(
    post,
    path = "/api/v1/wallet/connect",
    tag = "Contributor",
    security(("bearer_auth" = [])),
    request_body = ConnectWalletBody,
    responses(
        (status = 200, description = "Payout wallet saved for the authenticated contributor", body = OkResponse),
        (status = 401, description = "Missing or invalid bearer token", body = ErrorResponse),
        (status = 500, description = "Failed to save wallet", body = ErrorResponse)
    )
)]
pub(crate) async fn connect_wallet(
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

    if payout_chain.eq_ignore_ascii_case("stellar") {
        crate::infra::stellar::accounts::require_usdc_payout_account(&state, &payout_address)
            .await?;
    }

    upsert_contributor_wallet(
        &state,
        user.github_id,
        user.github_username.as_deref().unwrap_or(""),
        &payout_chain,
        &payout_address,
    )
    .await?;

    // Update email if provided
    if let Some(email) = user.email {
        if !email.is_empty() {
            let _ = crate::modules::contributor::repository::set_profile_email(
                &state,
                user.github_id,
                &email,
            )
            .await;
        }
    }

    resume_parked_bounties(&state, user.github_id).await;

    Ok(Json(OkResponse { ok: true }))
}

async fn resume_parked_bounties(state: &AppState, github_id: i64) {
    let profile = match get_profile_by_github_id(state, github_id).await {
        Ok(Some(p)) => p,
        Ok(None) => return,
        Err(error) => {
            warn!(%error, "failed to load profile after wallet connect");
            return;
        }
    };

    let bounties = match list_bounties_for_contributor(state, profile.id).await {
        Ok(b) => b,
        Err(error) => {
            warn!(%error, "failed to list bounties after wallet connect");
            return;
        }
    };

    for bounty in bounties {
        if bounty.status == "paid" || bounty.status == "cancelled" {
            continue;
        }

        match state
            .queue
            .enqueue_advance_issue(BountyJobData::new(bounty.id, "wallet-connected"))
            .await
        {
            Ok(outcome) => info!(
                issue = bounty.github_issue_number,
                outcome = outcome.label(),
                "wallet connected; bounty automation resumed"
            ),
            Err(error) => {
                warn!(%error, issue = bounty.github_issue_number, "failed to resume bounty automation")
            }
        }
    }
}

#[utoipa::path(
    get,
    path = "/api/v1/contributor/me",
    tag = "Contributor",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "Authenticated contributor profile and bounties", body = ContributorMeResponse),
        (status = 401, description = "Missing or invalid bearer token", body = ErrorResponse),
        (status = 500, description = "Failed to load contributor", body = ErrorResponse)
    )
)]
pub(crate) async fn get_contributor_me(
    State(state): State<AppState>,
    user: AuthedUser,
) -> Result<Json<ContributorMeResponse>, AppError> {
    use crate::modules::contributor::repository::get_wallets_for_profile;

    let profile = get_profile_by_github_id(&state, user.github_id).await?;

    let Some(mut profile) = profile else {
        return Ok(Json(ContributorMeResponse { contributor: None }));
    };

    // Update email if provided and different
    if let Some(email) = &user.email {
        if !email.is_empty() && profile.email.as_ref() != Some(email) {
            let _ = crate::modules::contributor::repository::set_profile_email(
                &state,
                user.github_id,
                email,
            )
            .await;
            profile.email = Some(email.clone());
        }
    }

    let wallets = get_wallets_for_profile(&state, profile.id).await?;
    let bounties = list_bounties_for_contributor(&state, profile.id).await?;

    let primary_wallet = wallets.iter().find(|w| w.is_primary);
    let stellar_wallet = wallets
        .iter()
        .find(|w| w.chain == "stellar")
        .map(|w| w.address.clone());

    let bounty_rows: Vec<serde_json::Value> = bounties
        .iter()
        .map(|b| {
            serde_json::json!({
                "id": b.id,
                "repo_id": b.repo_id,
                "github_issue_id": b.github_issue_id,
                "github_issue_number": b.github_issue_number,
                "title": b.title,
                "reward_amount": b.reward_amount,
                "status": b.status,
                "assigned_at": b.assigned_at,
                "merged_at": b.merged_at,
                "paid_at": b.paid_at,
                "created_at": b.created_at,
            })
        })
        .collect();

    let contributor_json = serde_json::json!({
        "id": profile.id,
        "github_id": profile.github_id,
        "username": profile.username,
        "full_name": profile.full_name,
        "avatar_url": profile.avatar_url,
        "stellar_wallet": stellar_wallet,
        "payout_chain": primary_wallet.map(|w| &w.chain),
        "payout_address": primary_wallet.map(|w| &w.address),
        "created_at": profile.created_at,
        "bounties": bounty_rows,
    });

    Ok(Json(ContributorMeResponse {
        contributor: Some(contributor_json),
    }))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/wallet/connect", post(connect_wallet))
        .route("/api/v1/contributor/me", get(get_contributor_me))
}
