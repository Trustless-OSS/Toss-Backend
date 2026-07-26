// @generated automatically by Diesel CLI.

diesel::table! {
    assignments (id) {
        id -> Uuid,
        issue_id -> Uuid,
        contributor_id -> Nullable<Uuid>,
        assigned_at -> Nullable<Timestamptz>,
        pr_number -> Nullable<Int4>,
        pr_merged_at -> Nullable<Timestamptz>,
        payout_status -> Text,
        completion_percentage -> Nullable<Numeric>,
    }
}

diesel::table! {
    contributors (id) {
        id -> Uuid,
        github_user_id -> Int8,
        github_username -> Text,
        stellar_wallet -> Nullable<Text>,
        payout_chain -> Nullable<Text>,
        payout_address -> Nullable<Text>,
        created_at -> Nullable<Timestamptz>,
    }
}

diesel::table! {
    issues (id) {
        id -> Uuid,
        repo_id -> Uuid,
        github_issue_id -> Int8,
        github_issue_number -> Int4,
        title -> Text,
        reward_amount -> Numeric,
        difficulty_label -> Nullable<Text>,
        milestone_index -> Nullable<Int4>,
        status -> Text,
        created_at -> Nullable<Timestamptz>,
    }
}

diesel::table! {
    repos (id) {
        id -> Uuid,
        github_repo_id -> Int8,
        github_installation_id -> Nullable<Int8>,
        full_name -> Text,
        owner_github_id -> Int8,
        owner_username -> Text,
        owner_type -> Nullable<Text>,
        installer_github_id -> Nullable<Int8>,
        is_fork -> Nullable<Bool>,
        is_private -> Nullable<Bool>,
        escrow_contract_id -> Nullable<Text>,
        escrow_balance -> Numeric,
        reward_low -> Numeric,
        reward_medium -> Numeric,
        reward_high -> Numeric,
        created_at -> Nullable<Timestamptz>,
        escrow_funder_wallet -> Nullable<Text>,
    }
}

diesel::table! {
    webhook_deliveries (id) {
        id -> Uuid,
        delivery_id -> Text,
        event -> Text,
        action -> Nullable<Text>,
        status -> Text,
        job_id -> Nullable<Text>,
        attempts -> Int4,
        first_attempt_at -> Timestamptz,
        last_attempt_at -> Timestamptz,
        completed_at -> Nullable<Timestamptz>,
        last_error -> Nullable<Text>,
        correlation_id -> Nullable<Text>,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
    }
}

diesel::joinable!(assignments -> contributors (contributor_id));
diesel::joinable!(assignments -> issues (issue_id));
diesel::joinable!(issues -> repos (repo_id));

diesel::allow_tables_to_appear_in_same_query!(
    assignments,
    contributors,
    issues,
    repos,
    webhook_deliveries,
);
