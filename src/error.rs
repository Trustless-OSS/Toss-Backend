use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};
use thiserror::Error;
use utoipa::openapi::ServerBuilder;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("{message}")]
    BadRequest { message: String },
    #[error("{message}")]
    Unauthorized { message: String },
    #[error("{message}")]
    Forbidden { message: String },
    #[error("{message}")]
    NotFound { message: String },
    #[error("{message}")]
    WebhookError {
        message: String,
        context: Option<Value>,
    },
    #[error("{message}")]
    StellarError {
        message: String,
        context: Option<Value>,
    },
    #[error("{message}")]
    GitHubError {
        message: String,
        context: Option<Value>,
    },
    #[error("{message}")]
    DatabaseError {
        message: String,
        context: Option<Value>,
    },
    #[error("{message}")]
    Internal { message: String },

    #[error("{message}")]
    EnvVarError { message :  String},
}

impl AppError {
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::BadRequest {
            message: message.into(),
        }
    }

    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::Unauthorized {
            message: message.into(),
        }
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::Forbidden {
            message: message.into(),
        }
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::NotFound {
            message: message.into(),
        }
    }

    pub fn webhook(message: impl Into<String>) -> Self {
        Self::WebhookError {
            message: message.into(),
            context: None,
        }
    }

    pub fn stellar(message: impl Into<String>) -> Self {
        Self::StellarError {
            message: message.into(),
            context: None,
        }
    }

    pub fn github(message: impl Into<String>) -> Self {
        Self::GitHubError {
            message: message.into(),
            context: None,
        }
    }

    pub fn database(message: impl Into<String>) -> Self {
        Self::DatabaseError {
            message: message.into(),
            context: None,
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal {
            message: message.into(),
        }
    }

    pub fn EnvVarError(name : impl Into<String>) -> Self {
        Self::EnvVarError{message : name.into()}
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = match &self {
            Self::BadRequest { .. } => StatusCode::BAD_REQUEST,
            Self::Unauthorized { .. } => StatusCode::UNAUTHORIZED,
            Self::Forbidden { .. } => StatusCode::FORBIDDEN,
            Self::NotFound { .. } => StatusCode::NOT_FOUND,
            Self::WebhookError { .. }
            | Self::StellarError { .. }
            | Self::GitHubError { .. }
            | Self::DatabaseError { .. }
            | Self::Internal { .. }
            | Self::EnvVarError { .. } => StatusCode::INTERNAL_SERVER_ERROR,
        };

        let message = self.to_string();
        if status.is_server_error() {
            tracing::error!(%status, error = %message, "request failed");
        }

        (status, Json(json!({ "error": message }))).into_response()
    }
}

pub fn unauthorized_response() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({ "error": "Unauthorized" })),
    )
        .into_response()
}

pub fn require_db(pool: &Option<sqlx::PgPool>) -> Result<&sqlx::PgPool, AppError> {
    pool.as_ref()
        .ok_or_else(|| AppError::internal("Database is not configured"))
}
