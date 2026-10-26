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
        (name = "Notifications", description = "In-app and email notifications"),
        (name = "Tickets", description = "Authenticated support ticket lifecycle"),
        (name = "GitHub", description = "GitHub App webhook ingestion"),
        (name = "Dashboard", description = "Maintainer dashboard metrics and public leaderboard")
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
        crate::modules::repo::handlers::list_installation_repos,
        crate::modules::repo::handlers::sync_installation,
        crate::modules::repo::handlers::list_issues,
        crate::modules::repo::handlers::repo_details,
        crate::modules::repo::handlers::update_rewards,
        crate::modules::repo::handlers::delete_repo,
        crate::modules::contributor::routes::connect_wallet,
        crate::modules::contributor::routes::get_contributor_me,
        crate::modules::bounty::handler::push_milestone,
        crate::modules::bounty::handler::retry_issue,
        crate::modules::bounty::handler::reject_issue,
        crate::modules::bounty::handler::hold_issue,
        crate::modules::escrow::handler::create_escrow_unsigned,
        crate::modules::escrow::handler::submit_deploy,
        crate::modules::escrow::handler::fund_unsigned,
        crate::modules::escrow::handler::submit_fund,
        crate::modules::escrow::handler::refund,
        crate::modules::escrow::handler::close_unsigned,
        crate::modules::escrow::handler::submit_close,
        crate::modules::notification::handlers::list_notifications,
        crate::modules::notification::handlers::unread_count,
        crate::modules::notification::handlers::read_all,
        crate::modules::notification::handlers::read_one,
        crate::modules::ticket::handler::create_ticket,
        crate::modules::ticket::handler::get_ticket,
        crate::modules::ticket::handler::patch_ticket,
        crate::modules::ticket::handler::resolve_ticket,
        crate::modules::github::routes::handle_github_webhook,
        crate::modules::dashboard::handlers::activity,
        crate::modules::dashboard::handlers::summary,
        crate::modules::dashboard::handlers::funds_movement,
        crate::modules::dashboard::handlers::maintainer_leaderboard,
        crate::modules::dashboard::handlers::public_leaderboard,
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
        crate::shared::models::Repo,
        crate::shared::models::Bounty,
        crate::shared::models::Profile,
        crate::shared::models::Wallet,
        crate::modules::repo::model::RepoResponse,
        crate::modules::repo::model::RepoDetails,
        crate::modules::repo::model::InstallationRepoSummary,
        crate::modules::repo::model::InstallationReposList,
        crate::modules::repo::model::SyncInstallationResult,
        crate::modules::repo::model::OkResponse,
        crate::modules::repo::datatypes::ConnectRepo,
        crate::modules::repo::datatypes::UpdateRewards,
        crate::modules::repo::datatypes::SyncInstallation,
        crate::modules::contributor::model::ConnectWalletBody,
        crate::modules::contributor::model::ContributorMeResponse,
        crate::modules::bounty::model::Milestone,
        crate::modules::bounty::model::MilestoneResponse,
        crate::modules::bounty::model::RetryIssueResponse,
        crate::modules::bounty::model::RejectIssueResponse,
        crate::modules::bounty::model::HoldIssueResponse,
        crate::modules::escrow::dto::CreateEscrow,
        crate::modules::escrow::dto::SubmitDeploy,
        crate::modules::escrow::dto::FundEscrow,
        crate::modules::escrow::dto::SubmitFund,
        crate::modules::escrow::dto::RefundEscrow,
        crate::modules::escrow::dto::CloseEscrow,
        crate::modules::escrow::dto::SubmitClose,
        crate::modules::escrow::dto::UnsignedTransactionResponse,
        crate::modules::escrow::dto::ContractIdResponse,
        crate::modules::escrow::dto::SubmitFundResponse,
        crate::modules::escrow::dto::RefundResponse,
        crate::modules::notification::handlers::NotificationResponse,
        crate::modules::notification::handlers::UnreadCountResponse,
        crate::modules::ticket::dtype::CreateTicketRequest,
        crate::modules::ticket::dtype::PatchTicketRequest,
        crate::modules::ticket::dtype::TicketResponse,
        crate::modules::github::routes::GitHubWebhookPayload,
        crate::modules::dashboard::model::ActivityRow,
        crate::modules::dashboard::model::ActivityKind,
        crate::modules::dashboard::model::FundsMovementPoint,
        crate::modules::dashboard::model::DashboardSummary,
        crate::modules::dashboard::model::LeaderboardEntry,
        crate::modules::dashboard::model::LeaderboardPage,
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
        "/api/v1/health",
        "/api/v1/health/database",
        "/api/v1/health/redis",
        "/api/v1/health/trustless-work",
        "/api/v1/queue/stats",
        "/api/v1/repos",
        "/api/v1/repos/connect",
        "/api/v1/repos/installation-repos",
        "/api/v1/repos/sync-installation",
        "/api/v1/repos/{repoId}",
        "/api/v1/repos/{repoId}/issues",
        "/api/v1/repos/{repoId}/rewards",
        "/api/v1/wallet/connect",
        "/api/v1/contributor/me",
        "/api/v1/milestones/push",
        "/api/v1/issues/{issueId}/retry",
        "/api/v1/issues/{issueId}/reject",
        "/api/v1/issues/{issueId}/hold",
        "/api/v1/escrow/create-unsigned",
        "/api/v1/escrow/submit-deploy",
        "/api/v1/escrow/fund-unsigned",
        "/api/v1/escrow/submit-fund",
        "/api/v1/escrow/refund",
        "/api/v1/escrow/close-unsigned",
        "/api/v1/escrow/submit-close",
        "/api/v1/notifications",
        "/api/v1/notifications/unread-count",
        "/api/v1/notifications/read-all",
        "/api/v1/notifications/{id}/read",
        "/api/v1/tickets",
        "/api/v1/tickets/{ticketId}",
        "/api/v1/tickets/{ticketId}/resolve",
        "/api/v1/webhooks/github",
        "/api/v1/dashboard/activity",
        "/api/v1/dashboard/summary",
        "/api/v1/dashboard/funds-movement",
        "/api/v1/dashboard/leaderboard",
        "/api/leaderboard",
    ];

    const AUTHENTICATED_OPERATIONS: &[(&str, &str)] = &[
        ("/api/v1/repos", "get"),
        ("/api/v1/repos/connect", "post"),
        ("/api/v1/repos/installation-repos", "get"),
        ("/api/v1/repos/sync-installation", "post"),
        ("/api/v1/repos/{repoId}", "get"),
        ("/api/v1/repos/{repoId}", "delete"),
        ("/api/v1/repos/{repoId}/rewards", "put"),
        ("/api/v1/wallet/connect", "post"),
        ("/api/v1/contributor/me", "get"),
        ("/api/v1/milestones/push", "post"),
        ("/api/v1/issues/{issueId}/retry", "post"),
        ("/api/v1/issues/{issueId}/reject", "post"),
        ("/api/v1/issues/{issueId}/hold", "post"),
        ("/api/v1/escrow/create-unsigned", "post"),
        ("/api/v1/escrow/submit-deploy", "post"),
        ("/api/v1/escrow/fund-unsigned", "post"),
        ("/api/v1/escrow/submit-fund", "post"),
        ("/api/v1/escrow/refund", "post"),
        ("/api/v1/escrow/close-unsigned", "post"),
        ("/api/v1/escrow/submit-close", "post"),
        ("/api/v1/notifications", "get"),
        ("/api/v1/notifications/unread-count", "get"),
        ("/api/v1/notifications/read-all", "post"),
        ("/api/v1/notifications/{id}/read", "post"),
        ("/api/v1/tickets", "post"),
        ("/api/v1/tickets/{ticketId}", "get"),
        ("/api/v1/tickets/{ticketId}", "patch"),
        ("/api/v1/tickets/{ticketId}/resolve", "post"),
        ("/api/v1/dashboard/activity", "get"),
        ("/api/v1/dashboard/summary", "get"),
        ("/api/v1/dashboard/funds-movement", "get"),
        ("/api/v1/dashboard/leaderboard", "get"),
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
                "patch" => item.patch.as_ref(),
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
        let item = spec.paths.paths.get("/api/v1/repos/{repoId}").unwrap();
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
            "CreateEscrow",
            "UnsignedTransactionResponse",
            "GitHubWebhookPayload",
        ] {
            assert!(schemas.contains_key(name), "missing schema {name}");
        }
    }
}
