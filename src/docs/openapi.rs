use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    paths(
        crate::routes::health::health_handler,
        crate::routes::health::database_health_handler,
        crate::routes::health::redis_health_handler,
        crate::routes::health::trustless_work_health_handler,
    ),
    components(schemas(
        crate::routes::health::HealthResponse,
        crate::routes::health::ShuttingDownResponse,
        crate::routes::health::DependencyHealthResponse,
    ))
)]
pub struct ApiDoc;
