use rust_decimal::Decimal;
use serde::Deserialize;
use utoipa::{IntoParams, ToSchema};

#[derive(Debug, Deserialize, ToSchema)]
#[schema(as = ConnectRepoBody)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConnectRepo {
    pub gh_repo_id: i64,
    pub full_name: String,
    pub owner_gh_id: i64,
    pub owner_username: String,
    pub gh_token: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub(crate) struct UpdateRewards {
    pub label: String,
    pub amount: Decimal,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SyncInstallation {
    #[serde(alias = "installation_id")]
    pub installation_id: i64,
    #[serde(default, alias = "githubRepoId", alias = "github_repo_id")]
    pub gh_repo_id: Option<i64>,
    #[serde(default, alias = "githubRepoIds", alias = "github_repo_ids")]
    pub gh_repo_ids: Option<Vec<i64>>,
}

#[derive(Debug, Deserialize, IntoParams, ToSchema)]
#[into_params(parameter_in = Query)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InstallationReposQuery {
    #[serde(alias = "installation_id")]
    pub installation_id: i64,
}
