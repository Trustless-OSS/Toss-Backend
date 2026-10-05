use crate::error::{is_unique_violation, map_db_err, require_db, AppError};
use crate::shared::models::{entities::Notification, schema};
use crate::state::AppState;
use uuid::Uuid;

use super::kinds::Delivery;

pub struct Repository;

impl Repository {
    pub async fn insert(
        state: &AppState,
        n: &super::kinds::Notify,
        delivery: &Delivery,
    ) -> Result<Option<Notification>, AppError> {
        let mut db = require_db(&state.db)?;
        match toasty::create!(schema::Notification {
            profile_id: n.recipient,
            kind: n.kind.as_str().to_string(),
            title: n.title.clone(),
            body: Some(n.body.clone()),
            ref_id: n.ref_id,
            dedupe_key: n.dedupe_key.clone(),
            data: Some(n.data.clone()),
            email_status: match delivery {
                Delivery::BellAndEmail => "pending".to_string(),
                _ => "not_requested".to_string(),
            },
        })
        .exec(&mut db)
        .await
        {
            Ok(row) => Ok(Some(row.into())),
            Err(e) if is_unique_violation(&e) => Ok(None),
            Err(e) => Err(map_db_err(e)),
        }
    }

    pub async fn get(state: &AppState, id: Uuid) -> Result<Option<schema::Notification>, AppError> {
        let mut db = require_db(&state.db)?;

        schema::Notification::filter_by_id(id)
            .first()
            .exec(&mut db)
            .await
            .map_err(map_db_err)
    }

    pub async fn set_email_status(
        state: &AppState,
        id: Uuid,
        status: &str,
        send: bool,
    ) -> Result<(), AppError> {
        let mut db = require_db(&state.db)?;

        if send {
            toasty::update!(schema::Notification::filter_by_id(id){
                email_status : status.to_string() ,
                email_sent_at : Some(jiff::Timestamp::now())
            })
            .exec(&mut db)
            .await
            .map_err(map_db_err)
        } else {
            toasty::update!(schema::Notification::filter_by_id(id){
                email_status : status.to_string()
            })
            .exec(&mut db)
            .await
            .map_err(map_db_err)
        }
    }

    pub async fn list(
        state: &AppState,
        profile_id: Uuid,
        unread_only: bool,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<Notification>, AppError> {
        let mut db = require_db(&state.db)?;

        let mut list = schema::Notification::filter_by_profile_id(profile_id)
            .order_by(schema::Notification::fields().created_at().desc())
            .exec(&mut db)
            .await
            .map_err(map_db_err)?;

        // Filter by is_read if needed
        if unread_only {
            list.retain(|n| !n.is_read);
        }

        let list = list
            .into_iter()
            .skip(offset.max(0) as usize)
            .take(limit.max(1) as usize)
            .map(Notification::from)
            .collect();

        Ok(list)
    }

    pub async fn unread_count(state: &AppState, profile_id: Uuid) -> Result<i64, AppError> {
        let mut db = require_db(&state.db)?;

        let list = schema::Notification::filter_by_profile_id(profile_id)
            .exec(&mut db)
            .await
            .map_err(map_db_err)?;

        let count = list.iter().filter(|n| !n.is_read).count();

        Ok(count as i64)
    }

    pub async fn mark_read(
        state: &AppState,
        profile_id: Uuid,
        id: Option<Uuid>,
    ) -> Result<(), AppError> {
        let mut db = require_db(&state.db)?;

        if let Some(id) = id {
            toasty::update!(schema::Notification::filter_by_id(id).filter_by_profile_id(profile_id) {
                is_read: true
            })
            .exec(&mut db)
            .await
            .map_err(map_db_err)?;

            Ok(())
        } else {
            toasty::update!(schema::Notification::filter_by_profile_id(profile_id) {
                is_read: true
            })
            .exec(&mut db)
            .await
            .map_err(map_db_err)?;

            Ok(())
        }
    }
}
