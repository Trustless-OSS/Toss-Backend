use crate::error::{is_unique_violation, map_db_err, require_db, AppError};
use crate::shared::models::{entities::Notification, schema};
use crate::state::AppState;
use uuid::Uuid;

pub struct Repository;

impl Repository {
    pub async fn insert(
        state: &AppState,
        n: &super::kinds::Notify,
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
            email_status: if n.email { "pending" } else { "not_requested" }.to_string(),
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

        let list = schema::Notification::filter_by_profile_id(profile_id)
            .order_by(schema::Notification::fields().created_at().desc())
            .limit(limit.max(1) as usize)
            .offset(offset.max(0) as usize)
            .exec(&mut db)
            .await
            .map_err(map_db_err)?;

        Ok(list
            .into_iter()
            .filter(|n| !unread_only || !n.is_read)
            .map(Notification::from)
            .collect())
    }

    pub async fn unread_count(state: &AppState, profile_id: Uuid) -> Result<i64, AppError> {
        let mut db = require_db(&state.db)?;

        let x = schema::Notification::filter_by_profile_id(profile_id)
            .exec(&mut db)
            .await
            .map_err(map_db_err)?;

        Ok(x.into_iter().filter(|n| !n.is_read).count() as i64)
    }

    pub async fn mark_read(
        state: &AppState,
        profile_id: Uuid,
        id: Option<Uuid>,
    ) -> Result<(), AppError> {
        let mut db = require_db(&state.db)?;

        if let Some(id) = id {
            toasty::update!(schema::Notification::filter_by_id(id) {
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
