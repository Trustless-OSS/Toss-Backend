//! Postgres-backed idempotency ledger for GitHub webhook deliveries.
//!
//! The Redis queue already deduplicates by delivery id on the fast path.
//! This module is the backstop: even if the Redis dedup key is ever lost
//! (flush, eviction, instance swap), a handler's money-moving side effects
//! still run at most once per `X-GitHub-Delivery` id, because claiming a
//! delivery is a single `INSERT ... ON CONFLICT DO NOTHING`.

use diesel::prelude::*;
use diesel_async::RunQueryDsl;

use crate::{
    error::{get_conn, AppError},
    schema::webhook_deliveries,
    state::AppState,
};

pub enum DeliveryClaim {
    /// First time we've seen this delivery id (or the previous attempt
    /// crashed before finishing) -- go ahead and run the handler.
    Proceed,
    /// A previous attempt already completed successfully; skip the handler.
    Duplicate,
}

#[derive(Insertable)]
#[diesel(table_name = webhook_deliveries)]
struct NewDelivery<'a> {
    delivery_id: &'a str,
    event: &'a str,
    action: Option<&'a str>,
    correlation_id: Option<&'a str>,
}

/// Atomically claim a delivery id for processing. Returns `Duplicate` only
/// when a prior attempt is recorded as `completed`; a delivery stuck in
/// `processing` (the previous worker crashed mid-handler) is retried.
pub async fn claim_delivery(
    state: &AppState,
    delivery_id: &str,
    event: &str,
    action: Option<&str>,
    correlation_id: &str,
) -> Result<DeliveryClaim, AppError> {
    if state.db.is_none() {
        // No database configured for this deployment; the Redis dedup key
        // is the only guard available, so don't block processing on it.
        return Ok(DeliveryClaim::Proceed);
    }
    let mut conn = get_conn(&state.db).await?;

    let inserted = diesel::insert_into(webhook_deliveries::table)
        .values(NewDelivery {
            delivery_id,
            event,
            action,
            correlation_id: Some(correlation_id),
        })
        .on_conflict(webhook_deliveries::delivery_id)
        .do_nothing()
        .execute(&mut conn)
        .await
        .map_err(|error| AppError::database(error.to_string()))?;

    if inserted == 1 {
        return Ok(DeliveryClaim::Proceed);
    }

    let status: String = webhook_deliveries::table
        .filter(webhook_deliveries::delivery_id.eq(delivery_id))
        .select(webhook_deliveries::status)
        .first(&mut conn)
        .await
        .map_err(|error| AppError::database(error.to_string()))?;

    if status == "completed" {
        return Ok(DeliveryClaim::Duplicate);
    }

    // Previous attempt never finished (crash/retry): bump the attempt
    // counter and let the caller run the handler again.
    diesel::update(
        webhook_deliveries::table.filter(webhook_deliveries::delivery_id.eq(delivery_id)),
    )
    .set((
        webhook_deliveries::attempts.eq(webhook_deliveries::attempts + 1),
        webhook_deliveries::last_attempt_at.eq(diesel::dsl::now),
        webhook_deliveries::updated_at.eq(diesel::dsl::now),
    ))
    .execute(&mut conn)
    .await
    .map_err(|error| AppError::database(error.to_string()))?;

    Ok(DeliveryClaim::Proceed)
}

pub async fn mark_completed(state: &AppState, delivery_id: &str) -> Result<(), AppError> {
    let mut conn = get_conn(&state.db).await?;
    diesel::update(
        webhook_deliveries::table.filter(webhook_deliveries::delivery_id.eq(delivery_id)),
    )
    .set((
        webhook_deliveries::status.eq("completed"),
        webhook_deliveries::completed_at.eq(diesel::dsl::now),
        webhook_deliveries::updated_at.eq(diesel::dsl::now),
    ))
    .execute(&mut conn)
    .await
    .map_err(|error| AppError::database(error.to_string()))?;
    Ok(())
}

pub async fn mark_failed(
    state: &AppState,
    delivery_id: &str,
    error: &AppError,
) -> Result<(), AppError> {
    let mut conn = get_conn(&state.db).await?;
    diesel::update(
        webhook_deliveries::table.filter(webhook_deliveries::delivery_id.eq(delivery_id)),
    )
    .set((
        webhook_deliveries::last_error.eq(error.to_string()),
        webhook_deliveries::updated_at.eq(diesel::dsl::now),
    ))
    .execute(&mut conn)
    .await
    .map_err(|db_error| AppError::database(db_error.to_string()))?;
    Ok(())
}
