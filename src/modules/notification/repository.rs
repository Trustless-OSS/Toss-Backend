use super::kinds::{Kind, Notify};
use crate::error::AppError;
use crate::state::AppState;
use uuid::Uuid;

pub struct Repository;

impl Repository {
    pub async fn insert(state: &AppState, nofify: Notify) -> Result<(), AppError> {
        todo!()
    }

    pub async fn get(state: &AppState, id: Uuid) -> Result<Notify, AppError> {
        todo!()
    }
}
