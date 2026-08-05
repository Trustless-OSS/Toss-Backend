use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    paths(crate::routes::health::health_handler),
    components(schemas(
        crate::routes::health::HealthResponse,
        crate::routes::health::ShuttingDownResponse
    ))
)]
pub struct ApiDoc;
