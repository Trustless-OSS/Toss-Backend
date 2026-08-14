use rust_decimal::Decimal;
use uuid::Uuid;

use crate::{
    error::{is_unique_violation, map_db_err, require_db, AppError},
    modules::{
        contributor::repository::get_contributor_by_id,
        escrow::repository::refund_repo_balance,
        repo::repository::{get_repo_by_id, invalidate_repo_cache},
    },
    shared::models::{schema, Assignment, Contributor, Issue, Repo},
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
    let issue = schema::Issue::filter_by_repo_id_and_github_issue_id(repo_id, github_issue_id)
        .first()
        .exec(&mut db)
        .await
        .map_err(map_db_err)?;

    let Some(issue) = issue else {
        return Ok(false);
    };

    let assignment = schema::Assignment::filter_by_issue_id(issue.id)
        .first()
        .exec(&mut db)
        .await
        .map_err(map_db_err)?;

    let Some(assignment) = assignment else {
        return Ok(false);
    };

    let Some(contributor_id) = assignment.contributor_id else {
        return Ok(false);
    };

    let contributor = get_contributor_by_id(state, contributor_id).await?;
    Ok(contributor.is_some_and(|c| c.github_user_id == github_user_id))
}

pub async fn list_issues_for_repo(
    state: &AppState,
    repo_id: Uuid,
    limit: i64,
    offset: i64,
) -> Result<(Vec<serde_json::Value>, i64), AppError> {
    let mut db = require_db(&state.db)?;
    let limit = limit.max(0) as usize;
    let offset = offset.max(0) as usize;

    let all = schema::Issue::filter_by_repo_id(repo_id)
        .exec(&mut db)
        .await
        .map_err(map_db_err)?;
    let total = all.len() as i64;

    let issues = schema::Issue::filter_by_repo_id(repo_id)
        .order_by(schema::Issue::fields().created_at().desc())
        .limit(limit)
        .offset(offset)
        .exec(&mut db)
        .await
        .map_err(map_db_err)?;

    let mut rows = Vec::with_capacity(issues.len());
    for issue in issues {
        let issue_dto = Issue::from(issue);
        let assignments = schema::Assignment::filter_by_issue_id(issue_dto.id)
            .exec(&mut db)
            .await
            .map_err(map_db_err)?;

        let mut assignment_rows = Vec::new();
        for assignment in assignments {
            let assignment = Assignment::from(assignment);
            let contributor = if let Some(contributor_id) = assignment.contributor_id {
                get_contributor_by_id(state, contributor_id).await?
            } else {
                None
            };

            assignment_rows.push(serde_json::json!({
                "id": assignment.id,
                "issue_id": assignment.issue_id,
                "contributor_id": assignment.contributor_id,
                "assigned_at": assignment.assigned_at,
                "pr_number": assignment.pr_number,
                "pr_merged_at": assignment.pr_merged_at,
                "payout_status": assignment.payout_status,
                "completion_percentage": assignment.completion_percentage,
                "contributors": contributor,
            }));
        }

        rows.push(serde_json::json!({
            "id": issue_dto.id,
            "repo_id": issue_dto.repo_id,
            "github_issue_id": issue_dto.github_issue_id,
            "github_issue_number": issue_dto.github_issue_number,
            "title": issue_dto.title,
            "reward_amount": issue_dto.reward_amount,
            "difficulty_label": issue_dto.difficulty_label,
            "milestone_index": issue_dto.milestone_index,
            "status": issue_dto.status,
            "created_at": issue_dto.created_at,
            "assignments": assignment_rows,
        }));
    }

    Ok((rows, total))
}

pub async fn get_issue_by_id(state: &AppState, issue_id: Uuid) -> Result<Option<Issue>, AppError> {
    let mut db = require_db(&state.db)?;
    Ok(schema::Issue::filter_by_id(issue_id)
        .first()
        .exec(&mut db)
        .await
        .map_err(map_db_err)?
        .map(Issue::from))
}

pub async fn get_issue_with_repo(
    state: &AppState,
    issue_id: Uuid,
) -> Result<Option<(Issue, Repo)>, AppError> {
    let issue = get_issue_by_id(state, issue_id).await?;
    let Some(issue) = issue else {
        return Ok(None);
    };
    let repo = get_repo_by_id(state, issue.repo_id).await?;
    Ok(repo.map(|repo| (issue, repo)))
}

pub async fn get_assignment_for_issue(
    state: &AppState,
    issue_id: Uuid,
) -> Result<Option<(Assignment, Option<Contributor>)>, AppError> {
    let mut db = require_db(&state.db)?;
    let assignment = schema::Assignment::filter_by_issue_id(issue_id)
        .first()
        .exec(&mut db)
        .await
        .map_err(map_db_err)?;

    let Some(assignment) = assignment else {
        return Ok(None);
    };
    let assignment = Assignment::from(assignment);

    let contributor = if let Some(contributor_id) = assignment.contributor_id {
        get_contributor_by_id(state, contributor_id).await?
    } else {
        None
    };

    Ok(Some((assignment, contributor)))
}

pub async fn get_issue_by_repo_and_github_id(
    state: &AppState,
    repo_id: Uuid,
    github_issue_id: i64,
) -> Result<Option<Issue>, AppError> {
    let mut db = require_db(&state.db)?;
    Ok(
        schema::Issue::filter_by_repo_id_and_github_issue_id(repo_id, github_issue_id)
            .first()
            .exec(&mut db)
            .await
            .map_err(map_db_err)?
            .map(Issue::from),
    )
}

pub async fn get_issue_by_repo_and_number(
    state: &AppState,
    repo_id: Uuid,
    github_issue_number: i32,
) -> Result<Option<Issue>, AppError> {
    let mut db = require_db(&state.db)?;
    Ok(schema::Issue::filter(
        schema::Issue::fields().repo_id().eq(repo_id).and(
            schema::Issue::fields()
                .github_issue_number()
                .eq(github_issue_number),
        ),
    )
    .first()
    .exec(&mut db)
    .await
    .map_err(map_db_err)?
    .map(Issue::from))
}

pub async fn update_issue_status(
    state: &AppState,
    issue_id: Uuid,
    status: &str,
    milestone_index: Option<i32>,
) -> Result<(), AppError> {
    let mut db = require_db(&state.db)?;
    if let Some(milestone_index) = milestone_index {
        toasty::update!(schema::Issue::filter_by_id(issue_id) {
            status: status.to_string(),
            milestone_index: Some(milestone_index),
        })
        .exec(&mut db)
        .await
        .map_err(map_db_err)?;
    } else {
        toasty::update!(schema::Issue::filter_by_id(issue_id) {
            status: status.to_string(),
        })
        .exec(&mut db)
        .await
        .map_err(map_db_err)?;
    }
    Ok(())
}

pub async fn fail_assignments_for_issues(
    state: &AppState,
    issue_ids: &[Uuid],
) -> Result<(), AppError> {
    if issue_ids.is_empty() {
        return Ok(());
    }

    let mut db = require_db(&state.db)?;
    for issue_id in issue_ids {
        toasty::update!(schema::Assignment::filter_by_issue_id(issue_id) {
            payout_status: "failed".to_string(),
        })
        .exec(&mut db)
        .await
        .map_err(map_db_err)?;
    }
    Ok(())
}

pub async fn update_assignment_payout_status(
    state: &AppState,
    assignment_id: Uuid,
    payout_status: &str,
) -> Result<(), AppError> {
    let mut db = require_db(&state.db)?;
    toasty::update!(schema::Assignment::filter_by_id(assignment_id) {
        payout_status: payout_status.to_string(),
    })
    .exec(&mut db)
    .await
    .map_err(map_db_err)?;
    Ok(())
}

pub async fn list_issues_to_cancel(
    state: &AppState,
    repo_id: Uuid,
) -> Result<Vec<Issue>, AppError> {
    let mut db = require_db(&state.db)?;
    Ok(schema::Issue::filter(
        schema::Issue::fields().repo_id().eq(repo_id).and(
            schema::Issue::fields()
                .status()
                .eq("pending".to_string())
                .or(schema::Issue::fields().status().eq("active".to_string())),
        ),
    )
    .exec(&mut db)
    .await
    .map_err(map_db_err)?
    .into_iter()
    .map(Issue::from)
    .collect())
}

pub async fn create_issue_and_reserve_balance(
    state: &AppState,
    repo: &Repo,
    github_issue_id: i64,
    github_issue_number: i32,
    title: &str,
    reward_amount: Decimal,
    difficulty_label: &str,
) -> Result<Option<Issue>, AppError> {
    let mut db = require_db(&state.db)?;
    let mut tx = db.transaction().await.map_err(map_db_err)?;

    let mut schema_repo = schema::Repo::get_by_id(&mut tx, &repo.id)
        .await
        .map_err(map_db_err)?;

    if schema_repo.escrow_balance < reward_amount {
        return Err(AppError::bad_request("Insufficient escrow balance"));
    }

    let new_balance = round_balance(schema_repo.escrow_balance - reward_amount);
    toasty::update!(schema_repo {
        escrow_balance: new_balance
    })
    .exec(&mut tx)
    .await
    .map_err(map_db_err)?;

    let issue = toasty::create!(schema::Issue {
        repo_id: repo.id,
        github_issue_id,
        github_issue_number,
        title: title.to_string(),
        reward_amount,
        difficulty_label: Some(difficulty_label.to_string()),
        status: "pending".to_string(),
    })
    .exec(&mut tx)
    .await;

    let issue = match issue {
        Ok(issue) => issue,
        Err(error) if is_unique_violation(&error) => {
            return Ok(None);
        }
        Err(error) => return Err(map_db_err(error)),
    };

    tx.commit().await.map_err(map_db_err)?;
    invalidate_repo_cache(state, repo.id, Some(repo.github_repo_id)).await;
    Ok(Some(Issue::from(issue)))
}

pub async fn update_pending_issue_reward(
    state: &AppState,
    repo: &Repo,
    issue_id: Uuid,
    reward_amount: Decimal,
    difficulty_label: &str,
) -> Result<bool, AppError> {
    let mut db = require_db(&state.db)?;
    let mut tx = db.transaction().await.map_err(map_db_err)?;

    let mut issue = match schema::Issue::filter_by_id(issue_id)
        .first()
        .exec(&mut tx)
        .await
        .map_err(map_db_err)?
    {
        Some(issue) if issue.repo_id == repo.id => issue,
        _ => return Ok(false),
    };

    if issue.status != "pending" {
        return Ok(false);
    }

    let difference = reward_amount - issue.reward_amount;
    let mut schema_repo = schema::Repo::get_by_id(&mut tx, &repo.id)
        .await
        .map_err(map_db_err)?;

    if difference > Decimal::ZERO && schema_repo.escrow_balance < difference {
        return Ok(false);
    }

    let new_balance = round_balance(schema_repo.escrow_balance - difference);
    toasty::update!(schema_repo {
        escrow_balance: new_balance
    })
    .exec(&mut tx)
    .await
    .map_err(map_db_err)?;

    toasty::update!(issue {
        reward_amount,
        difficulty_label: Some(difficulty_label.to_string()),
    })
    .exec(&mut tx)
    .await
    .map_err(map_db_err)?;

    tx.commit().await.map_err(map_db_err)?;
    invalidate_repo_cache(state, repo.id, Some(repo.github_repo_id)).await;
    Ok(true)
}

pub async fn cancel_issue(state: &AppState, issue_id: Uuid) -> Result<(), AppError> {
    update_issue_status(state, issue_id, "cancelled", None).await
}

pub async fn complete_issue(state: &AppState, issue_id: Uuid) -> Result<(), AppError> {
    update_issue_status(state, issue_id, "completed", None).await
}

pub async fn reset_issue_to_pending(state: &AppState, issue_id: Uuid) -> Result<(), AppError> {
    let mut db = require_db(&state.db)?;
    toasty::update!(schema::Issue::filter_by_id(issue_id) {
        status: "pending".to_string(),
        milestone_index: Option::<i32>::None,
    })
    .exec(&mut db)
    .await
    .map_err(map_db_err)?;
    Ok(())
}

pub async fn delete_assignments_for_issue(
    state: &AppState,
    issue_id: Uuid,
) -> Result<(), AppError> {
    let mut db = require_db(&state.db)?;
    schema::Assignment::filter_by_issue_id(issue_id)
        .delete()
        .exec(&mut db)
        .await
        .map_err(map_db_err)?;
    Ok(())
}

pub async fn upsert_assignment(
    state: &AppState,
    issue_id: Uuid,
    contributor_id: Uuid,
) -> Result<(), AppError> {
    let mut db = require_db(&state.db)?;
    schema::Assignment::upsert_by_issue_id(issue_id)
        .contributor_id(Some(contributor_id))
        .exec(&mut db)
        .await
        .map_err(map_db_err)?;
    Ok(())
}

pub async fn update_assignment_completion_percentage(
    state: &AppState,
    assignment_id: Uuid,
    percentage: Decimal,
) -> Result<(), AppError> {
    let mut db = require_db(&state.db)?;
    toasty::update!(schema::Assignment::filter_by_id(assignment_id) {
        completion_percentage: Some(percentage),
    })
    .exec(&mut db)
    .await
    .map_err(map_db_err)?;
    Ok(())
}

pub async fn update_assignment_pr_merge(
    state: &AppState,
    assignment_id: Uuid,
    pr_number: i32,
) -> Result<(), AppError> {
    let mut db = require_db(&state.db)?;
    toasty::update!(schema::Assignment::filter_by_id(assignment_id) {
        pr_number: Some(pr_number),
        pr_merged_at: Some(jiff::Timestamp::now()),
    })
    .exec(&mut db)
    .await
    .map_err(map_db_err)?;
    Ok(())
}

pub async fn cancel_bounty_with_refund(
    state: &AppState,
    repo: &Repo,
    issue_id: Uuid,
    reward_amount: Decimal,
) -> Result<(), AppError> {
    cancel_issue(state, issue_id).await?;
    delete_assignments_for_issue(state, issue_id).await?;
    refund_repo_balance(state, repo, reward_amount).await
}
