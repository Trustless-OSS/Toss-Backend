use utoipa::{
    openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme},
    Modify, OpenApi,
};

struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        if let Some(components) = openapi.components.as_mut() {
            components.add_security_scheme(
                "bearer_auth",
                SecurityScheme::Http(
                    HttpBuilder::new()
                        .scheme(HttpAuthScheme::Bearer)
                        .bearer_format("JWT")
                        .description(Some(
                            "Supabase access token. Send as `Authorization: Bearer <token>`.",
                        ))
                        .build(),
                ),
            );
        }
    }
}

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Toss Backend API",
        description = "Trustless OSS backend: GitHub bounties, contributor wallets, and Stellar escrow.",
        version = "1.0.0"
    ),
    modifiers(&SecurityAddon),
    tags(
        (name = "Root", description = "Service root"),
        (name = "Health", description = "Liveness and dependency checks"),
        (name = "Queue", description = "Background job queue statistics"),
        (name = "Repos", description = "GitHub repository connection, issues, and rewards"),
        (name = "Contributor", description = "Contributor profile and payout wallet"),
        (name = "Bounty", description = "Milestones and issue bounty retries"),
        (name = "Escrow", description = "Stellar escrow deploy, fund, refund, and close"),
        (name = "GitHub", description = "GitHub App webhook ingestion")
    ),
    paths(
        crate::app::root_handler,
        crate::routes::health::health_handler,
        crate::routes::health::api_health,
        crate::routes::health::database_health_handler,
        crate::routes::health::redis_health_handler,
        crate::routes::health::trustless_work_health_handler,
        crate::routes::queue::queue_stats_handler,
        crate::modules::repo::handlers::list_repos,
        crate::modules::repo::handlers::connect_repo,
        crate::modules::repo::handlers::sync_installation,
        crate::modules::repo::handlers::list_issues,
        crate::modules::repo::handlers::repo_details,
        crate::modules::repo::handlers::update_rewards,
        crate::modules::repo::handlers::delete_repo,
        crate::modules::contributor::routes::connect_wallet,
        crate::modules::contributor::routes::get_contributor_me,
        crate::modules::bounty::routes::push_milestone,
        crate::modules::bounty::routes::retry_issue,
        crate::modules::escrow::handler::create_escrow_unsigned,
        crate::modules::escrow::handler::submit_deploy,
        crate::modules::escrow::handler::fund_unsigned,
        crate::modules::escrow::handler::submit_fund,
        crate::modules::escrow::handler::refund,
        crate::modules::escrow::handler::close_unsigned,
        crate::modules::escrow::handler::submit_close,
        crate::modules::github::routes::handle_github_webhook,
    ),
    components(schemas(
        crate::error::ErrorResponse,
        crate::app::RootResponse,
        crate::routes::health::HealthResponse,
        crate::routes::health::ShuttingDownResponse,
        crate::routes::health::DependencyHealthResponse,
        crate::routes::queue::QueueCounts,
        crate::routes::queue::QueueStatsResponse,
        crate::shared::pagination::PaginationQuery,
        crate::shared::pagination::PaginatedResponse<crate::shared::models::Repo>,
        crate::shared::pagination::PaginatedResponse<crate::shared::models::domain::IssueWithRelations>,
        crate::shared::models::Repo,
        crate::shared::models::Contributor,
        crate::shared::models::Issue,
        crate::shared::models::Assignment,
        crate::shared::models::domain::IssueWithRelations,
        crate::shared::models::domain::AssignmentWithContributor,
        crate::modules::repo::model::RepoResponse,
        crate::modules::repo::model::RepoDetails,
        crate::modules::repo::model::SyncInstallationResult,
        crate::modules::repo::model::OkResponse,
        crate::modules::repo::handlers::ConnectRepoBody,
        crate::modules::repo::handlers::UpdateRewardsBody,
        crate::modules::repo::handlers::SyncInstallationBody,
        crate::modules::contributor::model::ConnectWalletBody,
        crate::modules::contributor::model::ContributorMeResponse,
        crate::modules::contributor::model::ContributorProfile,
        crate::modules::contributor::model::ContributorAssignmentView,
        crate::modules::bounty::model::Milestone,
        crate::modules::bounty::model::MilestoneResponse,
        crate::modules::bounty::model::RetryIssueResponse,
        crate::modules::escrow::dto::CreateEscrowBody,
        crate::modules::escrow::dto::SubmitDeployBody,
        crate::modules::escrow::dto::FundEscrowBody,
        crate::modules::escrow::dto::SubmitFundBody,
        crate::modules::escrow::dto::RefundEscrowBody,
        crate::modules::escrow::dto::CloseEscrowBody,
        crate::modules::escrow::dto::SubmitCloseBody,
        crate::modules::escrow::dto::UnsignedTransactionResponse,
        crate::modules::escrow::dto::ContractIdResponse,
        crate::modules::escrow::dto::SubmitFundResponse,
        crate::modules::escrow::dto::RefundResponse,
        crate::modules::github::routes::GitHubWebhookPayload,
    ))
)]
pub struct ApiDoc;

#[cfg(test)]
mod tests {
    use super::*;
    use utoipa::OpenApi;

    const EXPECTED_PATHS: &[&str] = &[
        "/",
        "/health",
        "/api/health",
        "/api/health/database",
        "/api/health/redis",
        "/api/health/trustless-work",
        "/api/queue/stats",
        "/api/repos",
        "/api/repos/connect",
        "/api/repos/sync-installation",
        "/api/repos/{repoId}",
        "/api/repos/{repoId}/issues",
        "/api/repos/{repoId}/rewards",
        "/api/wallet/connect",
        "/api/contributor/me",
        "/api/milestones/push",
        "/api/issues/{issueId}/retry",
        "/api/escrow/create-unsigned",
        "/api/escrow/submit-deploy",
        "/api/escrow/fund-unsigned",
        "/api/escrow/submit-fund",
        "/api/escrow/refund",
        "/api/escrow/close-unsigned",
        "/api/escrow/submit-close",
        "/api/webhooks/github",
    ];

    const AUTHENTICATED_OPERATIONS: &[(&str, &str)] = &[
        ("/api/repos", "get"),
        ("/api/repos/connect", "post"),
        ("/api/repos/sync-installation", "post"),
        ("/api/repos/{repoId}", "get"),
        ("/api/repos/{repoId}", "delete"),
        ("/api/repos/{repoId}/rewards", "put"),
        ("/api/wallet/connect", "post"),
        ("/api/contributor/me", "get"),
        ("/api/milestones/push", "post"),
        ("/api/issues/{issueId}/retry", "post"),
        ("/api/escrow/create-unsigned", "post"),
        ("/api/escrow/submit-deploy", "post"),
        ("/api/escrow/fund-unsigned", "post"),
        ("/api/escrow/submit-fund", "post"),
        ("/api/escrow/refund", "post"),
        ("/api/escrow/close-unsigned", "post"),
        ("/api/escrow/submit-close", "post"),
    ];

    #[test]
    fn documents_every_public_route() {
        let spec = ApiDoc::openapi();
        for path in EXPECTED_PATHS {
            assert!(
                spec.paths.paths.contains_key(*path),
                "OpenAPI is missing path {path}"
            );
        }
        assert_eq!(
            spec.paths.paths.len(),
            EXPECTED_PATHS.len(),
            "unexpected extra OpenAPI paths: {:?}",
            spec.paths
                .paths
                .keys()
                .filter(|path| !EXPECTED_PATHS.contains(&path.as_str()))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn registers_bearer_security_on_authenticated_routes() {
        let spec = ApiDoc::openapi();
        let components = spec.components.as_ref().expect("components");
        assert!(
            components.security_schemes.contains_key("bearer_auth"),
            "bearer_auth security scheme is missing"
        );

        for (path, method) in AUTHENTICATED_OPERATIONS {
            let item = spec
                .paths
                .paths
                .get(*path)
                .unwrap_or_else(|| panic!("missing path {path}"));
            let operation = match *method {
                "get" => item.get.as_ref(),
                "post" => item.post.as_ref(),
                "put" => item.put.as_ref(),
                "delete" => item.delete.as_ref(),
                other => panic!("unsupported method {other}"),
            }
            .unwrap_or_else(|| panic!("missing {method} on {path}"));

            let has_bearer = operation.security.as_ref().is_some_and(|requirements| {
                requirements.iter().any(|requirement| {
                    serde_json::to_value(requirement)
                        .ok()
                        .and_then(|value| value.get("bearer_auth").cloned())
                        .is_some()
                })
            });
            assert!(has_bearer, "{method} {path} is missing bearer_auth");
        }
    }

    #[test]
    fn repo_path_exposes_get_and_delete() {
        let spec = ApiDoc::openapi();
        let item = spec.paths.paths.get("/api/repos/{repoId}").unwrap();
        assert!(item.get.is_some());
        assert!(item.delete.is_some());
    }

    #[test]
    fn registers_core_component_schemas() {
        let spec = ApiDoc::openapi();
        let schemas = &spec.components.as_ref().expect("components").schemas;
        for name in [
            "ErrorResponse",
            "RootResponse",
            "HealthResponse",
            "QueueStatsResponse",
            "Repo",
            "RepoDetails",
            "ConnectRepoBody",
            "ContributorMeResponse",
            "Milestone",
            "CreateEscrowBody",
            "UnsignedTransactionResponse",
            "GitHubWebhookPayload",
        ] {
            assert!(schemas.contains_key(name), "missing schema {name}");
        }
    }
}
