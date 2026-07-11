use rust_decimal::Decimal;
use serde::Serialize;
use uuid::Uuid;

use crate::shared::models::Repo;

#[derive(Debug)]
pub(crate) struct ConnectRepoInput {
    pub(crate) github_repo_id: i64,
    pub(crate) full_name: String,
    pub(crate) owner_github_id: i64,
    pub(crate) owner_username: String,
    pub(crate) gh_token: String,
    pub(crate) webhook_url: String,
}

#[derive(Debug)]
pub(crate) struct SyncInstallationInput {
    pub(crate) installation_id: i64,
    pub(crate) installer_github_id: i64,
}

#[derive(Debug)]
pub(crate) struct UpdateRewardsInput {
    pub(crate) repo_id: Uuid,
    pub(crate) maintainer_github_id: i64,
    pub(crate) reward_low: Decimal,
    pub(crate) reward_medium: Decimal,
    pub(crate) reward_high: Decimal,
}

#[derive(Debug)]
pub(crate) struct RepoAccessInput {
    pub(crate) repo_id: Uuid,
    pub(crate) github_id: i64,
}

#[derive(Debug, Serialize)]
pub(crate) struct RepoResponse {
    pub(crate) repo: Repo,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RepoDetails {
    pub(crate) repo: Repo,
    pub(crate) is_maintainer: bool,
    pub(crate) escrow_deployed: bool,
    pub(crate) escrow_status: &'static str,
    pub(crate) can_deploy_escrow: bool,
    pub(crate) can_fund_escrow: bool,
    pub(crate) can_close_escrow: bool,
    pub(crate) can_refund_escrow: bool,
}

impl RepoDetails {
    pub(crate) fn new(repo: Repo, is_maintainer: bool) -> Self {
        let escrow_deployed = repo.escrow_contract_id.is_some();

        Self {
            repo,
            is_maintainer,
            escrow_deployed,
            escrow_status: if escrow_deployed {
                "deployed"
            } else {
                "not_deployed"
            },
            can_deploy_escrow: is_maintainer && !escrow_deployed,
            can_fund_escrow: is_maintainer && escrow_deployed,
            can_close_escrow: is_maintainer && escrow_deployed,
            can_refund_escrow: is_maintainer && escrow_deployed,
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct SyncInstallationResult {
    pub(crate) synced: usize,
}

#[derive(Debug, Serialize)]
pub(crate) struct OkResponse {
    pub(crate) ok: bool,
}
