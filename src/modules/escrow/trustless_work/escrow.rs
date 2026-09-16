use crate::error::AppError;
use crate::modules::github;
use crate::state::AppState;
use uuid::Uuid;

use super::unsigned_tx::UnsignedTx;

pub struct TrustlessWorkAPI {
    state: AppState,
}

impl TrustlessWorkAPI {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }

    pub async fn deploy(&self, repo_id: Uuid, signed_xdr: &str) -> Result<String, AppError> {
        todo!()
    }

    pub async fn fund(&self) -> Result<String, AppError> {
        todo!()
    }

    pub async fn update(&self) -> Result<String, AppError> {
        todo!()
    }

    pub async fn approve_milestone(&self) -> Result<String, AppError> {
        todo!()
    }

    pub async fn release_milestone(&self) -> Result<String, AppError> {
        todo!()
    }

    pub async fn close(&self) -> Result<String, AppError> {
        todo!()
    }
}
