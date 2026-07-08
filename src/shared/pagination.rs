use axum::extract::Query;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PaginationQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

impl PaginationQuery {
    pub fn resolve(&self) -> (i64, i64) {
        let limit = self.limit.unwrap_or(100).clamp(1, 200);
        let offset = self.offset.unwrap_or(0).max(0);
        (limit, offset)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PaginatedResponse<T> {
    pub data: Vec<T>,
    pub total_count: i64,
    pub limit: i64,
    pub offset: i64,
}

pub type PaginatedQuery = Query<PaginationQuery>;
