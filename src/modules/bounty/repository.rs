use rust_decimal::Decimal;
use uuid::Uuid;

use crate::{
    error::{is_unique_violation, map_db_err, require_db, AppError},
    modules::{
        escrow::repository::refund_repo_balance,
        repo::repository::{get_repo_by_id, invalidate_repo_cache},
    },
    shared::models::{schema, Bounty, Profile, Repo},
    state::AppState,
};

fn round_balance(value: Decimal) -> Decimal {
    value.round_dp(7)
}

pub async fn is_assigned_contributor(
    state: &AppState,
    github_user_id: i64,
    repo_id: Uuid,
    github_issue_id: i64,
) -> Result<bool, AppError> {
    let mut db = require_db(&state.db)?;
    let bounty = schema::Bounty::filter_by_github_issue_id(github_issue_id)
        .first()
        .exec(&mut db)
        .await
        .map_err(map_db_err)?
        .filter(|b| b.repo_id == repo_id);

    let Some(bounty) = bounty else {
        return Ok(false);
    };

    let Some(assignee_id) = bounty.assignee_id else {
        return Ok(false);
    };

    let profile = schema::Profile::filter_by_id(assignee_id)
        .first()
        .exec(&mut db)
        .await
        .map_err(map_db_err)?;

    Ok(profile.is_some_and(|p| p.github_id == github_user_id))
}

pub async fn list_bounties_for_repo(
    state: &AppState,
    repo_id: Uuid,
    limit: i64,
    offset: i64,
) -> Result<(Vec<serde_json::Value>, i64), AppError> {
    let mut db = require_db(&state.db)?;
    let limit = limit.max(0) as usize;
    let offset = offset.max(0) as usize;

    let all = schema::Bounty::filter_by_repo_id(repo_id)
        .exec(&mut db)
        .await
        .map_err(map_db_err)?;
    let total = all.len() as i64;

    let bounties = schema::Bounty::filter_by_repo_id(repo_id)
        .order_by(schema::Bounty::fields().created_at().desc())
        .limit(limit)
        .offset(offset)
        .exec(&mut db)
        .await
        .map_err(map_db_err)?;

    let mut rows = Vec::with_capacity(bounties.len());
    for bounty in bounties {
        let bounty_dto = Bounty::from(bounty);

        let assignee = if let Some(assignee_id) = bounty_dto.assignee_id {
            schema::Profile::filter_by_id(assignee_id)
                .first()
                .exec(&mut db)
                .await
                .map_err(map_db_err)?
                .map(Profile::from)
        } else {
            None
        };

        rows.push(serde_json::json!({
            "id": bounty_dto.id,
            "repo_id": bounty_dto.repo_id,
            "reward_level_id": bounty_dto.reward_level_id,
            "milestone_index": bounty_dto.milestone_index,
            "github_issue_id": bounty_dto.github_issue_id,
            "github_issue_number": bounty_dto.github_issue_number,
            "title": bounty_dto.title,
            "reward_amount": bounty_dto.reward_amount,
            "status": bounty_dto.status,
            "assignee_id": bounty_dto.assignee_id,
            "assigned_at": bounty_dto.assigned_at,
            "merged_at": bounty_dto.merged_at,
            "paid_at": bounty_dto.paid_at,
            "created_at": bounty_dto.created_at,
            "assignee": assignee,
        }));
    }

    Ok((rows, total))
}

pub async fn get_bounty_by_id(
    state: &AppState,
    bounty_id: Uuid,
) -> Result<Option<Bounty>, AppError> {
    let mut db = require_db(&state.db)?;
    Ok(schema::Bounty::filter_by_id(bounty_id)
        .first()
        .exec(&mut db)
        .await
        .map_err(map_db_err)?
        .map(Bounty::from))
}

pub async fn get_bounty_with_repo(
    state: &AppState,
    bounty_id: Uuid,
) -> Result<Option<(Bounty, Repo)>, AppError> {
    let bounty = get_bounty_by_id(state, bounty_id).await?;
    let Some(bounty) = bounty else {
        return Ok(None);
    };
    let repo = get_repo_by_id(state, bounty.repo_id).await?;
    Ok(repo.map(|repo| (bounty, repo)))
}

pub async fn get_assignee_for_bounty(
    state: &AppState,
    bounty_id: Uuid,
) -> Result<Option<Profile>, AppError> {
    let mut db = require_db(&state.db)?;
    let bounty = schema::Bounty::filter_by_id(bounty_id)
        .first()
        .exec(&mut db)
        .await
        .map_err(map_db_err)?;

    let Some(bounty) = bounty else {
        return Ok(None);
    };

    let Some(assignee_id) = bounty.assignee_id else {
        return Ok(None);
    };

    Ok(schema::Profile::filter_by_id(assignee_id)
        .first()
        .exec(&mut db)
        .await
        .map_err(map_db_err)?
        .map(Profile::from))
}

pub async fn get_bounty_by_repo_and_github_id(
    state: &AppState,
    repo_id: Uuid,
    github_issue_id: i64,
) -> Result<Option<Bounty>, AppError> {
    let mut db = require_db(&state.db)?;
    Ok(schema::Bounty::filter_by_github_issue_id(github_issue_id)
        .first()
        .exec(&mut db)
        .await
        .map_err(map_db_err)?
        .filter(|b| b.repo_id == repo_id)
        .map(Bounty::from))
}

pub async fn get_bounty_by_repo_and_number(
    state: &AppState,
    repo_id: Uuid,
    github_issue_number: i32,
) -> Result<Option<Bounty>, AppError> {
    let mut db = require_db(&state.db)?;
    Ok(schema::Bounty::filter(
        schema::Bounty::fields().repo_id().eq(repo_id).and(
            schema::Bounty::fields()
                .github_issue_number()
                .eq(github_issue_number),
        ),
    )
    .first()
    .exec(&mut db)
    .await
    .map_err(map_db_err)?
    .map(Bounty::from))
}

pub async fn update_bounty_status(
    state: &AppState,
    bounty_id: Uuid,
    status: &str,
    milestone_index: Option<i32>,
) -> Result<(), AppError> {
    let mut db = require_db(&state.db)?;
    if let Some(milestone_index) = milestone_index {
        toasty::update!(schema::Bounty::filter_by_id(bounty_id) {
            status: status.to_string(),
            milestone_index: Some(milestone_index),
        })
        .exec(&mut db)
        .await
        .map_err(map_db_err)?;
    } else {
        toasty::update!(schema::Bounty::filter_by_id(bounty_id) {
            status: status.to_string(),
        })
        .exec(&mut db)
        .await
        .map_err(map_db_err)?;
    }
    Ok(())
}

pub async fn list_bounties_to_cancel(
    state: &AppState,
    repo_id: Uuid,
) -> Result<Vec<Bounty>, AppError> {
    let mut db = require_db(&state.db)?;
    Ok(schema::Bounty::filter(
        schema::Bounty::fields().repo_id().eq(repo_id).and(
            schema::Bounty::fields()
                .status()
                .eq("open".to_string())
                .or(schema::Bounty::fields().status().eq("assigned".to_string())),
        ),
    )
    .exec(&mut db)
    .await
    .map_err(map_db_err)?
    .into_iter()
    .map(Bounty::from)
    .collect())
}

pub async fn create_bounty_and_reserve_balance(
    state: &AppState,
    repo: &Repo,
    github_issue_id: i64,
    github_issue_number: i32,
    title: &str,
    reward_amount: Decimal,
    reward_level_id: Option<Uuid>,
) -> Result<Option<Bounty>, AppError> {
    let mut db = require_db(&state.db)?;
    let mut tx = db.transaction().await.map_err(map_db_err)?;

    let mut schema_repo = schema::Repositories::get_by_id(&mut tx, &repo.id)
        .await
        .map_err(map_db_err)?;

    let current_balance = schema_repo.escrow_balance.unwrap_or(Decimal::ZERO);
    if current_balance < reward_amount {
        return Err(AppError::bad_request("Insufficient escrow balance"));
    }

    let new_balance = round_balance(current_balance - reward_amount);
    toasty::update!(schema_repo {
        escrow_balance: Some(new_balance),
    })
    .exec(&mut tx)
    .await
    .map_err(map_db_err)?;

    let bounty = toasty::create!(schema::Bounty {
        repo_id: repo.id,
        github_issue_id,
        github_issue_number,
        title: Some(title.to_string()),
        reward_amount: Some(reward_amount),
        reward_level_id,
        status: "open".to_string(),
    })
    .exec(&mut tx)
    .await;

    let bounty = match bounty {
        Ok(bounty) => bounty,
        Err(error) if is_unique_violation(&error) => {
            return Ok(None);
        }
        Err(error) => return Err(map_db_err(error)),
    };

    tx.commit().await.map_err(map_db_err)?;
    invalidate_repo_cache(state, repo.id, Some(repo.github_repo_id)).await;
    Ok(Some(Bounty::from(bounty)))
}

pub async fn update_pending_bounty_reward(
    state: &AppState,
    repo: &Repo,
    bounty_id: Uuid,
    reward_amount: Decimal,
    reward_level_id: Option<Uuid>,
) -> Result<bool, AppError> {
    let mut db = require_db(&state.db)?;
    let mut tx = db.transaction().await.map_err(map_db_err)?;

    let mut bounty = match schema::Bounty::filter_by_id(bounty_id)
        .first()
        .exec(&mut tx)
        .await
        .map_err(map_db_err)?
    {
        Some(bounty) if bounty.repo_id == repo.id => bounty,
        _ => return Ok(false),
    };

    if bounty.status != "open" {
        return Ok(false);
    }

    let old_amount = bounty.reward_amount.unwrap_or(Decimal::ZERO);
    let difference = reward_amount - old_amount;
    let mut schema_repo = schema::Repositories::get_by_id(&mut tx, &repo.id)
        .await
        .map_err(map_db_err)?;

    let current_balance = schema_repo.escrow_balance.unwrap_or(Decimal::ZERO);
    if difference > Decimal::ZERO && current_balance < difference {
        return Ok(false);
    }

    let new_balance = round_balance(current_balance - difference);
    toasty::update!(schema_repo {
        escrow_balance: Some(new_balance),
    })
    .exec(&mut tx)
    .await
    .map_err(map_db_err)?;

    toasty::update!(bounty {
        reward_amount: Some(reward_amount),
        reward_level_id,
    })
    .exec(&mut tx)
    .await
    .map_err(map_db_err)?;

    tx.commit().await.map_err(map_db_err)?;
    invalidate_repo_cache(state, repo.id, Some(repo.github_repo_id)).await;
    Ok(true)
}

pub async fn cancel_bounty(state: &AppState, bounty_id: Uuid) -> Result<(), AppError> {
    update_bounty_status(state, bounty_id, "cancelled", None).await
}

pub async fn complete_bounty(state: &AppState, bounty_id: Uuid) -> Result<(), AppError> {
    update_bounty_status(state, bounty_id, "paid", None).await
}

pub async fn reset_bounty_to_open(state: &AppState, bounty_id: Uuid) -> Result<(), AppError> {
    let mut db = require_db(&state.db)?;
    toasty::update!(schema::Bounty::filter_by_id(bounty_id) {
        status: "open".to_string(),
        milestone_index: Option::<i32>::None,
        assignee_id: Option::<Uuid>::None,
        assigned_at: Option::<jiff::Timestamp>::None,
        merged_at: Option::<jiff::Timestamp>::None,
    })
    .exec(&mut db)
    .await
    .map_err(map_db_err)?;
    Ok(())
}

pub async fn assign_bounty(
    state: &AppState,
    bounty_id: Uuid,
    assignee_id: Uuid,
) -> Result<(), AppError> {
    let mut db = require_db(&state.db)?;
    toasty::update!(schema::Bounty::filter_by_id(bounty_id) {
        assignee_id: Some(assignee_id),
        assigned_at: Some(jiff::Timestamp::now()),
        status: "assigned".to_string(),
    })
    .exec(&mut db)
    .await
    .map_err(map_db_err)?;
    Ok(())
}

pub async fn unassign_bounty(state: &AppState, bounty_id: Uuid) -> Result<(), AppError> {
    let mut db = require_db(&state.db)?;
    toasty::update!(schema::Bounty::filter_by_id(bounty_id) {
        assignee_id: Option::<Uuid>::None,
        assigned_at: Option::<jiff::Timestamp>::None,
        status: "open".to_string(),
    })
    .exec(&mut db)
    .await
    .map_err(map_db_err)?;
    Ok(())
}

pub async fn mark_bounty_merged(state: &AppState, bounty_id: Uuid) -> Result<(), AppError> {
    let mut db = require_db(&state.db)?;
    toasty::update!(schema::Bounty::filter_by_id(bounty_id) {
        merged_at: Some(jiff::Timestamp::now()),
        status: "merged".to_string(),
    })
    .exec(&mut db)
    .await
    .map_err(map_db_err)?;
    Ok(())
}

pub async fn cancel_bounty_with_refund(
    state: &AppState,
    repo: &Repo,
    bounty_id: Uuid,
    reward_amount: Option<Decimal>,
) -> Result<(), AppError> {
    cancel_bounty(state, bounty_id).await?;
    refund_repo_balance(state, repo, reward_amount).await
}

pub async fn fail_bounties_for_ids(state: &AppState, bounty_ids: &[Uuid]) -> Result<(), AppError> {
    if bounty_ids.is_empty() {
        return Ok(());
    }
    let mut db = require_db(&state.db)?;
    for bounty_id in bounty_ids {
        toasty::update!(schema::Bounty::filter_by_id(bounty_id) {
            status: "cancelled".to_string(),
        })
        .exec(&mut db)
        .await
        .map_err(map_db_err)?;
    }
    Ok(())
}

pub use assign_bounty as upsert_assignment;
pub use cancel_bounty as cancel_issue;
pub use complete_bounty as complete_issue;
pub use fail_bounties_for_ids as fail_assignments_for_issues;
pub use get_bounty_by_id as get_issue_by_id;
pub use get_bounty_by_repo_and_github_id as get_issue_by_repo_and_github_id;
pub use get_bounty_by_repo_and_number as get_issue_by_repo_and_number;
pub use get_bounty_with_repo as get_issue_with_repo;
pub use list_bounties_for_repo as list_issues_for_repo;
pub use list_bounties_to_cancel as list_issues_to_cancel;
pub use reset_bounty_to_open as reset_issue_to_pending;
pub use unassign_bounty as delete_assignments_for_issue;
pub use update_bounty_status as update_issue_status;

// create_issue_and_reserve_balance — old callers pass (state, repo, github_issue_id,
// github_issue_number, title, reward_amount, difficulty_label: &str).
// The new function takes reward_level_id: Option<Uuid> instead of difficulty_label.
// We keep a shim that accepts the string label and resolves the reward_level_id.
pub async fn create_issue_and_reserve_balance(
    state: &AppState,
    repo: &Repo,
    github_issue_id: i64,
    github_issue_number: i32,
    title: &str,
    reward_amount: Decimal,
    difficulty_label: &str,
) -> Result<Option<Bounty>, AppError> {
    let reward_level_id = resolve_reward_level_id(state, repo.id, difficulty_label).await?;
    create_bounty_and_reserve_balance(
        state,
        repo,
        github_issue_id,
        github_issue_number,
        title,
        reward_amount,
        reward_level_id,
    )
    .await
}

// update_pending_issue_reward — old callers pass difficulty_label: &str.
pub async fn update_pending_issue_reward(
    state: &AppState,
    repo: &Repo,
    bounty_id: Uuid,
    reward_amount: Decimal,
    difficulty_label: &str,
) -> Result<bool, AppError> {
    let reward_level_id = resolve_reward_level_id(state, repo.id, difficulty_label).await?;
    update_pending_bounty_reward(state, repo, bounty_id, reward_amount, reward_level_id).await
}

// update_assignment_completion_percentage — completion_percentage is removed from the
// new schema. This is a no-op shim so callers continue to compile.
pub async fn update_assignment_completion_percentage(
    _state: &AppState,
    _bounty_id: Uuid,
    _percentage: Decimal,
) -> Result<(), AppError> {
    Ok(())
}

// update_assignment_pr_merge — pr_number is removed from schema.
// We record the merge event by setting merged_at on the bounty.
pub async fn update_assignment_pr_merge(
    state: &AppState,
    bounty_id: Uuid,
    _pr_number: i32,
) -> Result<(), AppError> {
    mark_bounty_merged(state, bounty_id).await
}

async fn resolve_reward_level_id(
    state: &AppState,
    repo_id: Uuid,
    label: &str,
) -> Result<Option<Uuid>, AppError> {
    let mut db = require_db(&state.db)?;
    Ok(
        schema::Reward::filter_by_repo_id_and_label(repo_id, label.to_string())
            .first()
            .exec(&mut db)
            .await
            .map_err(map_db_err)?
            .map(|r| r.id),
    )
}

// update_assignment_payout_status now maps to updating the bounty paid_at + status
pub async fn update_assignment_payout_status(
    state: &AppState,
    bounty_id: Uuid,
    payout_status: &str,
) -> Result<(), AppError> {
    let mut db = require_db(&state.db)?;
    match payout_status {
        "released" => {
            toasty::update!(schema::Bounty::filter_by_id(bounty_id) {
                paid_at: Some(jiff::Timestamp::now()),
                status: "paid".to_string(),
            })
            .exec(&mut db)
            .await
            .map_err(map_db_err)?;
        }
        _ => {
            toasty::update!(schema::Bounty::filter_by_id(bounty_id) {
                status: payout_status.to_string(),
            })
            .exec(&mut db)
            .await
            .map_err(map_db_err)?;
        }
    }
    Ok(())
}

pub async fn get_assignment_for_issue(
    state: &AppState,
    bounty_id: Uuid,
) -> Result<Option<(Bounty, Option<Profile>)>, AppError> {
    let mut db = require_db(&state.db)?;
    let bounty = schema::Bounty::filter_by_id(bounty_id)
        .first()
        .exec(&mut db)
        .await
        .map_err(map_db_err)?;

    let Some(bounty) = bounty else {
        return Ok(None);
    };

    let assignee_id = bounty.assignee_id;
    let bounty = Bounty::from(bounty);

    let profile = if let Some(id) = assignee_id {
        schema::Profile::filter_by_id(id)
            .first()
            .exec(&mut db)
            .await
            .map_err(map_db_err)?
            .map(Profile::from)
    } else {
        None
    };

    Ok(Some((bounty, profile)))
}
