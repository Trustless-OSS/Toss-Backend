use crate::error::AppError;
use crate::state::AppState;

pub struct TrustlessWorkClient {
    state: AppState,
}

impl TrustlessWorkClient {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }

    pub async fn deploy(&self) -> Result<String, AppError> {
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
