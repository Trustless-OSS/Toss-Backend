use axum::extract::Query;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

#[derive(Debug, Clone, Default, Deserialize, IntoParams, ToSchema)]
#[into_params(parameter_in = Query)]
pub struct PaginationQuery {
    /// Page size. Defaults to 100, clamped to 1–200.
    pub limit: Option<i64>,
    /// Number of items to skip. Defaults to 0.
    pub offset: Option<i64>,
}

impl PaginationQuery {
    pub fn resolve(&self) -> (i64, i64) {
        let limit = self.limit.unwrap_or(100).clamp(1, 200);
        let offset = self.offset.unwrap_or(0).max(0);
        (limit, offset)
    }
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct PaginatedResponse<T: ToSchema> {
    pub data: Vec<T>,
    pub total_count: i64,
    pub limit: i64,
    pub offset: i64,
}

pub type PaginatedQuery = Query<PaginationQuery>;
