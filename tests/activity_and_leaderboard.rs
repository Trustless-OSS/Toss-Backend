use std::collections::HashSet;
use std::sync::Arc;

use axum::{
    extract::{rejection::QueryRejection, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde_json::{json, Value};
use toss_backend::{
    config::Config,
    docs::openapi::ApiDoc,
    error::{unauthorized_response, AppError},
    infra,
    modules::{
        activity::{
            dto::{
                ActivityEventKind, ActivityKindFilter, ActivityLedgerItem, ActivityQuery,
                ContributorLeaderboardEntry, LeaderboardQuery, PaginatedLedgerResponse,
                DEFAULT_ACTIVITY_LIMIT, DEFAULT_LEADERBOARD_LIMIT,
            },
            repository::{load_maintainer_event_batches, RawActivityEvent, RawPoolDeposit},
            service::{
                build_leaderboard, compute_total_pages, derive_events_from_bounties_and_deposits,
                flatten_batches, map_activity_event, paginate, query_activity,
                scope_events_to_repos,
            },
        },
        dashboard::repository::EnrichedBounty,
    },
    shared::models::schema,
    state::AppState,
};
use utoipa::OpenApi;
use uuid::Uuid;

fn ts(secs: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(secs, 0).expect("valid unix timestamp")
}

fn jiff_ts(secs: i64) -> jiff::Timestamp {
    jiff::Timestamp::from_second(secs).expect("valid jiff timestamp")
}

#[allow(clippy::too_many_arguments)]
fn sample_item(
    id: &str,
    secs: i64,
    repo: &str,
    issue_number: Option<i32>,
    issue_title: Option<&str>,
    kind: ActivityEventKind,
    amount_usdc: Option<i64>,
    contributor: Option<&str>,
    tx_hash: Option<&str>,
) -> ActivityLedgerItem {
    ActivityLedgerItem {
        id: id.to_string(),
        at: ts(secs),
        repo: repo.to_string(),
        issue_number,
        issue_title: issue_title.map(str::to_string),
        kind,
        amount_usdc: amount_usdc.map(Decimal::from),
        contributor: contributor.map(str::to_string),
        tx_hash: tx_hash.map(str::to_string),
    }
}

#[test]
fn pagination_math_handles_empty_boundaries_and_overflow() {
    assert_eq!(compute_total_pages(0, 20), 1);
    assert_eq!(compute_total_pages(1, 20), 1);
    assert_eq!(compute_total_pages(20, 20), 1);
    assert_eq!(compute_total_pages(21, 20), 2);
    assert_eq!(compute_total_pages(40, 20), 2);
    assert_eq!(compute_total_pages(41, 20), 3);
    assert_eq!(compute_total_pages(0, 50), 1);
    assert_eq!(compute_total_pages(50, 50), 1);
    assert_eq!(compute_total_pages(51, 50), 2);

    let items: Vec<ActivityLedgerItem> = (0..45)
        .map(|i| {
            sample_item(
                &format!("ev-{i:02}"),
                1_726_000_000 + i,
                "org/repo",
                Some(i as i32 + 1),
                Some("Issue"),
                ActivityEventKind::Rewarded,
                Some(10),
                None,
                None,
            )
        })
        .collect();

    let page1 = paginate(items.clone(), 1, 20);
    assert_eq!(page1.data.len(), 20);
    assert_eq!(page1.page, 1);
    assert_eq!(page1.limit, 20);
    assert_eq!(page1.total, 45);
    assert_eq!(page1.total_pages, 3);

    let page3 = paginate(items.clone(), 3, 20);
    assert_eq!(page3.data.len(), 5);
    assert_eq!(page3.page, 3);
    assert_eq!(page3.total, 45);
    assert_eq!(page3.total_pages, 3);

    let out_of_range = paginate(items.clone(), 4, 20);
    assert!(out_of_range.data.is_empty());
    assert_eq!(out_of_range.page, 4);
    assert_eq!(out_of_range.total, 45);
    assert_eq!(out_of_range.total_pages, 3);

    let huge_page = paginate(items, i64::MAX, 20);
    assert!(huge_page.data.is_empty());
    assert_eq!(huge_page.total, 45);
    assert_eq!(huge_page.total_pages, 3);
}

#[tokio::test]
async fn query_validation_defaults_and_rejects_invalid_inputs() {
    let default_activity = ActivityQuery::default().validate().unwrap();
    assert_eq!(default_activity.kind, ActivityKindFilter::All);
    assert_eq!(default_activity.repo, None);
    assert_eq!(default_activity.page, 1);
    assert_eq!(default_activity.limit, DEFAULT_ACTIVITY_LIMIT);

    let default_lb = LeaderboardQuery::default().validate().unwrap();
    assert_eq!(default_lb.repo, None);
    assert_eq!(default_lb.page, 1);
    assert_eq!(default_lb.limit, DEFAULT_LEADERBOARD_LIMIT);

    // Boundary max limit = 100 succeeds
    assert!(ActivityQuery {
        kind: Some("released".into()),
        repo: Some("Trustless-OSS/Toss-Backend".into()),
        page: Some(1),
        limit: Some(100),
    }
    .validate()
    .is_ok());

    // Unknown / malformed kinds -> 400 { "error": "..." }
    for bad_kind in ["", "RELEASED", "paid", "deposit", "unknown", "all "] {
        let err = ActivityQuery {
            kind: Some(bad_kind.into()),
            ..Default::default()
        }
        .validate()
        .unwrap_err();
        let resp = err.into_response();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "kind={bad_kind:?}");
        let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        let parsed: Value = serde_json::from_slice(&body).unwrap();
        assert!(parsed.get("error").and_then(Value::as_str).is_some());
    }

    // Invalid page and limit -> 400
    for bad_page in [0, -1, -99] {
        let err = ActivityQuery {
            page: Some(bad_page),
            ..Default::default()
        }
        .validate()
        .unwrap_err();
        assert_eq!(err.into_response().status(), StatusCode::BAD_REQUEST);

        let lb_err = LeaderboardQuery {
            page: Some(bad_page),
            ..Default::default()
        }
        .validate()
        .unwrap_err();
        assert_eq!(lb_err.into_response().status(), StatusCode::BAD_REQUEST);
    }

    for bad_limit in [0, -1, 101, 500] {
        let err = ActivityQuery {
            limit: Some(bad_limit),
            ..Default::default()
        }
        .validate()
        .unwrap_err();
        assert_eq!(err.into_response().status(), StatusCode::BAD_REQUEST);

        let lb_err = LeaderboardQuery {
            limit: Some(bad_limit),
            ..Default::default()
        }
        .validate()
        .unwrap_err();
        assert_eq!(lb_err.into_response().status(), StatusCode::BAD_REQUEST);
    }

    // Invalid repo format -> 400
    for bad_repo in [
        "",
        "   ",
        "noslash",
        "owner/",
        "/name",
        "a/b/c",
        "ow ner/repo",
    ] {
        let err = ActivityQuery {
            repo: Some(bad_repo.into()),
            ..Default::default()
        }
        .validate()
        .unwrap_err();
        assert_eq!(
            err.into_response().status(),
            StatusCode::BAD_REQUEST,
            "repo={bad_repo:?}"
        );
    }
}

#[test]
fn activity_filter_matrix_covers_every_kind_repo_and_stable_ordering() {
    let events = vec![
        sample_item(
            "ev-rewarded",
            1_726_000_100,
            "org/api",
            Some(1),
            Some("Rewarded issue"),
            ActivityEventKind::Rewarded,
            Some(100),
            None,
            None,
        ),
        sample_item(
            "ev-locked-pool",
            1_726_000_200,
            "org/api",
            None,
            None,
            ActivityEventKind::Locked,
            Some(1000),
            None,
            Some("tx_pool_01"),
        ),
        sample_item(
            "ev-assigned",
            1_726_000_300,
            "org/api",
            Some(1),
            Some("Rewarded issue"),
            ActivityEventKind::Assigned,
            None,
            Some("alice"),
            None,
        ),
        sample_item(
            "ev-unassigned",
            1_726_000_400,
            "org/web",
            Some(2),
            Some("Web issue"),
            ActivityEventKind::Unassigned,
            None,
            Some("bob"),
            None,
        ),
        sample_item(
            "ev-rejected",
            1_726_000_500,
            "org/web",
            Some(3),
            Some("Rejected issue"),
            ActivityEventKind::Rejected,
            None,
            Some("carol"),
            None,
        ),
        sample_item(
            "ev-released-b",
            1_726_000_600,
            "org/web",
            Some(4),
            Some("Merged web PR"),
            ActivityEventKind::Released,
            Some(250),
            Some("alice"),
            Some("tx_rel_b"),
        ),
        // Same timestamp as ev-released-b to verify deterministic id tie-breaking across pages
        sample_item(
            "ev-released-a",
            1_726_000_600,
            "org/api",
            Some(1),
            Some("Merged api PR"),
            ActivityEventKind::Released,
            Some(100),
            Some("alice"),
            Some("tx_rel_a"),
        ),
    ];

    // kind=all returns mixed kinds, newest first, deterministic id tie-break
    let all_query = ActivityQuery {
        kind: Some("all".into()),
        repo: None,
        page: Some(1),
        limit: Some(20),
    }
    .validate()
    .unwrap();
    let all = query_activity(events.clone(), &all_query);
    assert_eq!(all.total, 7);
    assert_eq!(all.total_pages, 1);
    assert_eq!(all.data[0].id, "ev-released-a");
    assert_eq!(all.data[1].id, "ev-released-b");
    assert!(all.data.windows(2).all(|w| w[0].at >= w[1].at));

    // Each individual kind returns strictly that kind
    for (kind_str, expected_kind, expected_count) in [
        ("rewarded", ActivityEventKind::Rewarded, 1),
        ("locked", ActivityEventKind::Locked, 1),
        ("released", ActivityEventKind::Released, 2),
        ("assigned", ActivityEventKind::Assigned, 1),
        ("unassigned", ActivityEventKind::Unassigned, 1),
        ("rejected", ActivityEventKind::Rejected, 1),
    ] {
        let q = ActivityQuery {
            kind: Some(kind_str.into()),
            repo: None,
            page: Some(1),
            limit: Some(20),
        }
        .validate()
        .unwrap();
        let page = query_activity(events.clone(), &q);
        assert_eq!(page.total, expected_count, "kind={kind_str}");
        assert!(page.data.iter().all(|row| row.kind == expected_kind));
    }

    // Combined kind + repo filter
    let repo_released = ActivityQuery {
        kind: Some("released".into()),
        repo: Some("org/api".into()),
        page: Some(1),
        limit: Some(20),
    }
    .validate()
    .unwrap();
    let filtered = query_activity(events.clone(), &repo_released);
    assert_eq!(filtered.total, 1);
    assert_eq!(filtered.data[0].id, "ev-released-a");
    assert_eq!(filtered.data[0].repo, "org/api");

    // Stable ids across pages with limit=2: no duplicates, exact full coverage
    let mut seen = Vec::new();
    for p in 1..=4 {
        let q = ActivityQuery {
            kind: Some("all".into()),
            repo: None,
            page: Some(p),
            limit: Some(2),
        }
        .validate()
        .unwrap();
        let page = query_activity(events.clone(), &q);
        assert_eq!(page.total, 7);
        assert_eq!(page.total_pages, 4);
        seen.extend(page.data.into_iter().map(|item| item.id));
    }
    assert_eq!(seen.len(), 7);
    let unique: HashSet<String> = seen.iter().cloned().collect();
    assert_eq!(unique.len(), 7);
}

#[test]
fn pool_deposits_map_as_locked_with_null_issue_fields_and_exact_json_contract() {
    // Path 1: `activity` event-store row with occurred_at + null issue fields
    let raw_pool = RawActivityEvent {
        id: Uuid::new_v4(),
        full_name: "Trustless-OSS/Toss-Backend".into(),
        event_type: "locked".into(),
        payload: Some(json!({
            "occurred_at": "2026-09-12T15:30:00Z",
            "issue_number": null,
            "issue_title": "Should be ignored when issue_number is null",
            "amount_usdc": 500,
            "contributor": null,
            "tx_hash": "9f86d081884c7d659a2feaa0c55ad015"
        })),
        actor_username: Some("maintainer_alice".into()),
        occurred_at: jiff_ts(1_700_000_000),
    };

    let mapped = map_activity_event(&raw_pool).expect("locked event should map");
    assert_eq!(mapped.kind, ActivityEventKind::Locked);
    assert_eq!(mapped.repo, "Trustless-OSS/Toss-Backend");
    assert_eq!(mapped.issue_number, None);
    assert_eq!(mapped.issue_title, None);
    assert_eq!(mapped.amount_usdc, Some(Decimal::from(500)));
    assert_eq!(mapped.contributor, None);
    assert_eq!(
        mapped.tx_hash.as_deref(),
        Some("9f86d081884c7d659a2feaa0c55ad015")
    );
    assert_eq!(mapped.at.to_rfc3339(), "2026-09-12T15:30:00+00:00");

    // Unrecognized event_type must never invent a kind
    let unknown_raw = RawActivityEvent {
        id: Uuid::new_v4(),
        full_name: "Trustless-OSS/Toss-Backend".into(),
        event_type: "custom_internal_event".into(),
        payload: None,
        actor_username: None,
        occurred_at: jiff_ts(1_700_000_000),
    };
    assert!(map_activity_event(&unknown_raw).is_none());

    // Path 2: Fallback derivation from EscrowFunder + Bounty
    let deposit_id = Uuid::new_v4();
    let bounty_id = Uuid::new_v4();
    let derived = derive_events_from_bounties_and_deposits(
        &[EnrichedBounty {
            id: bounty_id,
            repo_slug: "org/core".into(),
            issue_number: 42,
            issue_title: Some("Add ledger endpoint".into()),
            reward_amount: Some(Decimal::from(120)),
            status: "assigned".into(),
            contributor: Some("dev1".into()),
            milestone_index: Some(0),
            created_at: jiff_ts(1_726_000_000),
            assigned_at: Some(jiff_ts(1_726_000_100)),
            paid_at: None,
            payout_tx_hash: None,
        }],
        &[RawPoolDeposit {
            id: deposit_id,
            full_name: "org/core".into(),
            amount: Decimal::from(2500),
            tx_hash: "stellar_deposit_tx_hash".into(),
            funded_at: jiff_ts(1_726_000_200),
            contributor: None,
        }],
    );

    let pool_event = derived
        .iter()
        .find(|item| item.id == format!("deposit_{deposit_id}"))
        .expect("pool deposit must appear");
    assert_eq!(pool_event.kind, ActivityEventKind::Locked);
    assert_eq!(pool_event.issue_number, None);
    assert_eq!(pool_event.issue_title, None);
    assert_eq!(pool_event.amount_usdc, Some(Decimal::from(2500)));
    assert_eq!(
        pool_event.tx_hash.as_deref(),
        Some("stellar_deposit_tx_hash")
    );

    // Non-payout bounty events must not stub fake tx hashes
    for bounty_ev in derived
        .iter()
        .filter(|e| e.id != format!("deposit_{deposit_id}"))
    {
        assert_eq!(bounty_ev.tx_hash, None);
    }

    // Verify exact snake_case JSON wire format
    let envelope = query_activity(
        vec![pool_event.clone()],
        &ActivityQuery {
            kind: Some("locked".into()),
            ..Default::default()
        }
        .validate()
        .unwrap(),
    );
    let json_val = serde_json::to_value(&envelope).unwrap();
    assert_eq!(json_val["page"], 1);
    assert_eq!(json_val["limit"], 20);
    assert_eq!(json_val["total"], 1);
    assert_eq!(json_val["total_pages"], 1);

    let row = &json_val["data"][0];
    assert_eq!(row["repo"], "org/core");
    assert_eq!(row["kind"], "locked");
    assert!(row["issue_number"].is_null());
    assert!(row["issue_title"].is_null());
    assert_eq!(row["amount_usdc"], "2500");
    assert!(row["contributor"].is_null());
    assert_eq!(row["tx_hash"], "stellar_deposit_tx_hash");
    assert!(row["at"].is_string());
    assert!(row["id"].is_string());
}

#[test]
fn leaderboard_ranks_with_ties_unpaid_assignees_exclusions_and_sorted_repos() {
    let ev = |id: &str,
              secs: i64,
              repo: &str,
              kind: ActivityEventKind,
              amount: Option<i64>,
              contributor: &str| {
        sample_item(
            id,
            secs,
            repo,
            Some(1),
            Some("t"),
            kind,
            amount,
            Some(contributor),
            None,
        )
    };

    let events = vec![
        // alice: 250 USDC, 2 merged_prs, 3 assigned across org/web and org/api
        ev(
            "a1",
            100,
            "org/web",
            ActivityEventKind::Assigned,
            None,
            "alice",
        ),
        ev(
            "a2",
            101,
            "org/api",
            ActivityEventKind::Assigned,
            None,
            "alice",
        ),
        ev(
            "a3",
            102,
            "org/api",
            ActivityEventKind::Assigned,
            None,
            "alice",
        ),
        ev(
            "a4",
            103,
            "org/web",
            ActivityEventKind::Released,
            Some(100),
            "alice",
        ),
        ev(
            "a5",
            104,
            "org/api",
            ActivityEventKind::Released,
            Some(150),
            "alice",
        ),
        // bob: 100 USDC, 2 merged_prs (wins tie over carol & dave on merged_prs)
        ev(
            "b1",
            105,
            "org/api",
            ActivityEventKind::Assigned,
            None,
            "bob",
        ),
        ev(
            "b2",
            106,
            "org/api",
            ActivityEventKind::Assigned,
            None,
            "bob",
        ),
        ev(
            "b3",
            107,
            "org/api",
            ActivityEventKind::Released,
            Some(50),
            "bob",
        ),
        ev(
            "b4",
            108,
            "org/api",
            ActivityEventKind::Released,
            Some(50),
            "bob",
        ),
        // dave: 100 USDC, 1 merged_pr, 4 assigned (loses alphabetical tie to carol)
        ev(
            "d1",
            109,
            "org/web",
            ActivityEventKind::Assigned,
            None,
            "dave",
        ),
        ev(
            "d2",
            110,
            "org/web",
            ActivityEventKind::Assigned,
            None,
            "dave",
        ),
        ev(
            "d3",
            111,
            "org/web",
            ActivityEventKind::Assigned,
            None,
            "dave",
        ),
        ev(
            "d4",
            112,
            "org/web",
            ActivityEventKind::Assigned,
            None,
            "dave",
        ),
        ev(
            "d5",
            113,
            "org/web",
            ActivityEventKind::Released,
            Some(100),
            "dave",
        ),
        // carol: 100 USDC, 1 merged_pr, 1 assigned (wins alphabetical tie over dave)
        ev(
            "c1",
            114,
            "org/api",
            ActivityEventKind::Assigned,
            None,
            "carol",
        ),
        ev(
            "c2",
            115,
            "org/api",
            ActivityEventKind::Released,
            Some(100),
            "carol",
        ),
        // unpaid_dev: assigned only (0 earned, 0 merged_prs, 2 assigned) -> MUST appear
        ev(
            "u1",
            116,
            "org/web",
            ActivityEventKind::Assigned,
            None,
            "unpaid_dev",
        ),
        ev(
            "u2",
            117,
            "org/web",
            ActivityEventKind::Assigned,
            None,
            "unpaid_dev",
        ),
        // ghost: only rejected / unassigned / locked / rewarded -> MUST be absent
        ev(
            "g1",
            118,
            "org/api",
            ActivityEventKind::Rejected,
            None,
            "ghost",
        ),
        ev(
            "g2",
            119,
            "org/api",
            ActivityEventKind::Unassigned,
            None,
            "ghost",
        ),
        ev(
            "g3",
            120,
            "org/api",
            ActivityEventKind::Locked,
            Some(500),
            "ghost",
        ),
        ev(
            "g4",
            121,
            "org/api",
            ActivityEventKind::Rewarded,
            Some(500),
            "ghost",
        ),
    ];

    let full = build_leaderboard(&events, &LeaderboardQuery::default().validate().unwrap());
    assert_eq!(full.total, 5);
    assert_eq!(full.limit, 50);
    assert_eq!(full.total_pages, 1);

    let names: Vec<&str> = full
        .data
        .iter()
        .map(|e| e.github_username.as_str())
        .collect();
    assert_eq!(names, vec!["alice", "bob", "carol", "dave", "unpaid_dev"]);
    assert!(!names.contains(&"ghost"));

    assert_eq!(full.data[0].rank, 1);
    assert_eq!(full.data[0].earned_usdc, Decimal::from(250));
    assert_eq!(full.data[0].merged_prs, 2);
    assert_eq!(full.data[0].assigned, 3);
    assert_eq!(full.data[0].repos, vec!["org/api", "org/web"]);

    assert_eq!(full.data[4].rank, 5);
    assert_eq!(full.data[4].github_username, "unpaid_dev");
    assert_eq!(full.data[4].earned_usdc, Decimal::ZERO);
    assert_eq!(full.data[4].merged_prs, 0);
    assert_eq!(full.data[4].assigned, 2);
    assert_eq!(full.data[4].repos, vec!["org/web"]);

    // Verify 1-based global rank is preserved on page 2
    let page2 = build_leaderboard(
        &events,
        &LeaderboardQuery {
            repo: None,
            page: Some(2),
            limit: Some(2),
        }
        .validate()
        .unwrap(),
    );
    assert_eq!(page2.total, 5);
    assert_eq!(page2.total_pages, 3);
    assert_eq!(page2.data.len(), 2);
    assert_eq!(page2.data[0].rank, 3);
    assert_eq!(page2.data[0].github_username, "carol");
    assert_eq!(page2.data[1].rank, 4);
    assert_eq!(page2.data[1].github_username, "dave");

    // Verify exact snake_case JSON wire format
    let json_val = serde_json::to_value(&page2).unwrap();
    assert_eq!(json_val["page"], 2);
    assert_eq!(json_val["limit"], 2);
    assert_eq!(json_val["total"], 5);
    assert_eq!(json_val["total_pages"], 3);
    assert_eq!(json_val["data"][0]["rank"], 3);
    assert_eq!(json_val["data"][0]["github_username"], "carol");
    assert_eq!(json_val["data"][0]["earned_usdc"], "100");
    assert_eq!(json_val["data"][0]["merged_prs"], 1);
    assert_eq!(json_val["data"][0]["assigned"], 1);
    assert_eq!(json_val["data"][0]["repos"], json!(["org/api"]));
}

#[test]
fn empty_envelopes_return_total_zero_and_total_pages_one() {
    let empty_activity = query_activity(vec![], &ActivityQuery::default().validate().unwrap());
    let activity_json = serde_json::to_value(&empty_activity).unwrap();
    assert_eq!(
        activity_json,
        json!({
            "data": [],
            "page": 1,
            "limit": 20,
            "total": 0,
            "total_pages": 1
        })
    );

    let empty_lb = build_leaderboard(&[], &LeaderboardQuery::default().validate().unwrap());
    let lb_json = serde_json::to_value(&empty_lb).unwrap();
    assert_eq!(
        lb_json,
        json!({
            "data": [],
            "page": 1,
            "limit": 50,
            "total": 0,
            "total_pages": 1
        })
    );
}

#[test]
fn cross_maintainer_isolation_excludes_other_maintainers_events() {
    let all_events = vec![
        sample_item(
            "m1-rel",
            1_726_000_100,
            "maintainer-a/repo-a",
            Some(1),
            Some("Maintainer A issue"),
            ActivityEventKind::Released,
            Some(300),
            Some("contrib_a"),
            Some("tx_a"),
        ),
        sample_item(
            "m2-rel",
            1_726_000_200,
            "maintainer-b/secret-repo",
            Some(99),
            Some("Maintainer B private issue"),
            ActivityEventKind::Released,
            Some(900),
            Some("contrib_b"),
            Some("tx_b"),
        ),
    ];

    let maintainer_a_repos: HashSet<String> =
        ["maintainer-a/repo-a".to_string()].into_iter().collect();
    let scoped = scope_events_to_repos(all_events, &maintainer_a_repos);

    // Maintainer A activity feed only contains maintainer-a/repo-a
    let feed = query_activity(
        scoped.clone(),
        &ActivityQuery::default().validate().unwrap(),
    );
    assert_eq!(feed.total, 1);
    assert_eq!(feed.data[0].repo, "maintainer-a/repo-a");

    // Maintainer A requesting Maintainer B's repo explicitly sees empty 200 envelope
    let cross_repo_query = ActivityQuery {
        kind: Some("all".into()),
        repo: Some("maintainer-b/secret-repo".into()),
        page: Some(1),
        limit: Some(20),
    }
    .validate()
    .unwrap();
    let cross_feed = query_activity(scoped.clone(), &cross_repo_query);
    assert_eq!(cross_feed.total, 0);
    assert_eq!(cross_feed.total_pages, 1);
    assert!(cross_feed.data.is_empty());

    // Maintainer A leaderboard never exposes Maintainer B's contributors
    let lb = build_leaderboard(&scoped, &LeaderboardQuery::default().validate().unwrap());
    assert_eq!(lb.total, 1);
    assert_eq!(lb.data[0].github_username, "contrib_a");
}

#[tokio::test]
async fn http_layer_handles_auth_failures_invalid_queries_and_success_envelopes() {
    #[derive(Clone)]
    struct MockState {
        events: Arc<Vec<ActivityLedgerItem>>,
    }

    fn require_bearer(headers: &HeaderMap) -> Result<(), axum::response::Response> {
        let Some(raw) = headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
        else {
            return Err(unauthorized_response());
        };
        let Some((scheme, token)) = raw.split_once(' ') else {
            return Err(unauthorized_response());
        };
        if !scheme.eq_ignore_ascii_case("bearer") || token.trim() != "valid-test-token" {
            return Err(unauthorized_response());
        }
        Ok(())
    }

    async fn mock_activity(
        State(state): State<MockState>,
        headers: HeaderMap,
        query: Result<Query<ActivityQuery>, QueryRejection>,
    ) -> Result<Json<PaginatedLedgerResponse<ActivityLedgerItem>>, axum::response::Response> {
        require_bearer(&headers)?;
        let Query(raw) =
            query.map_err(|err| AppError::bad_request(err.to_string()).into_response())?;
        let validated = raw.validate().map_err(IntoResponse::into_response)?;
        Ok(Json(query_activity((*state.events).clone(), &validated)))
    }

    async fn mock_leaderboard(
        State(state): State<MockState>,
        headers: HeaderMap,
        query: Result<Query<LeaderboardQuery>, QueryRejection>,
    ) -> Result<Json<PaginatedLedgerResponse<ContributorLeaderboardEntry>>, axum::response::Response>
    {
        require_bearer(&headers)?;
        let Query(raw) =
            query.map_err(|err| AppError::bad_request(err.to_string()).into_response())?;
        let validated = raw.validate().map_err(IntoResponse::into_response)?;
        Ok(Json(build_leaderboard(&state.events, &validated)))
    }

    let state = MockState {
        events: Arc::new(vec![
            sample_item(
                "rel-1",
                1_726_000_500,
                "org/repo",
                Some(7),
                Some("Ship API"),
                ActivityEventKind::Released,
                Some(200),
                Some("alice"),
                Some("tx_abc"),
            ),
            sample_item(
                "asgn-1",
                1_726_000_400,
                "org/repo",
                Some(8),
                Some("Open task"),
                ActivityEventKind::Assigned,
                None,
                Some("bob"),
                None,
            ),
        ]),
    };

    let app = Router::new()
        .route("/api/activity", get(mock_activity))
        .route("/api/contributors/leaderboard", get(mock_leaderboard))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let client = reqwest::Client::new();
    let base = format!("http://{addr}");

    // Auth failures on both endpoints -> 401 { "error": "Unauthorized" }
    for path in ["/api/activity", "/api/contributors/leaderboard"] {
        let missing = client.get(format!("{base}{path}")).send().await.unwrap();
        assert_eq!(missing.status(), StatusCode::UNAUTHORIZED);
        let body: Value = missing.json().await.unwrap();
        assert_eq!(body, json!({ "error": "Unauthorized" }));

        let bad_token = client
            .get(format!("{base}{path}"))
            .header(header::AUTHORIZATION, "Bearer wrong-token")
            .send()
            .await
            .unwrap();
        assert_eq!(bad_token.status(), StatusCode::UNAUTHORIZED);
    }

    // Invalid kind & non-integer page -> 400 { "error": "..." }
    for invalid_url in [
        format!("{base}/api/activity?kind=invalid"),
        format!("{base}/api/activity?page=abc"),
        format!("{base}/api/activity?limit=101"),
        format!("{base}/api/contributors/leaderboard?page=0"),
        format!("{base}/api/contributors/leaderboard?limit=notanumber"),
    ] {
        let resp = client
            .get(&invalid_url)
            .header(header::AUTHORIZATION, "Bearer valid-test-token")
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "url={invalid_url}");
        let body: Value = resp.json().await.unwrap();
        assert!(
            body.get("error").and_then(Value::as_str).is_some(),
            "expected JSON error body for {invalid_url}, got {body}"
        );
    }

    // Valid filtered activity request
    let ok_activity = client
        .get(format!("{base}/api/activity?kind=released&page=1&limit=20"))
        .header(header::AUTHORIZATION, "Bearer valid-test-token")
        .send()
        .await
        .unwrap();
    assert_eq!(ok_activity.status(), StatusCode::OK);
    let act_body: Value = ok_activity.json().await.unwrap();
    assert_eq!(act_body["total"], 1);
    assert_eq!(act_body["total_pages"], 1);
    assert_eq!(act_body["data"][0]["kind"], "released");

    // Valid leaderboard request uses default limit=50
    let ok_lb = client
        .get(format!("{base}/api/contributors/leaderboard"))
        .header(header::AUTHORIZATION, "Bearer valid-test-token")
        .send()
        .await
        .unwrap();
    assert_eq!(ok_lb.status(), StatusCode::OK);
    let lb_body: Value = ok_lb.json().await.unwrap();
    assert_eq!(lb_body["limit"], 50);
    assert_eq!(lb_body["total"], 2);
    assert_eq!(lb_body["data"][0]["github_username"], "alice");
    assert_eq!(lb_body["data"][1]["github_username"], "bob");
    assert_eq!(lb_body["data"][1]["earned_usdc"], "0");

    server.abort();
}

#[test]
fn swagger_documents_activity_and_leaderboard_routes() {
    let spec = ApiDoc::openapi();

    for path in ["/api/activity", "/api/contributors/leaderboard"] {
        let item = spec
            .paths
            .paths
            .get(path)
            .unwrap_or_else(|| panic!("OpenAPI missing {path}"));
        let op = item
            .get
            .as_ref()
            .unwrap_or_else(|| panic!("OpenAPI missing GET on {path}"));

        let has_bearer = op.security.as_ref().is_some_and(|reqs| {
            reqs.iter().any(|req| {
                serde_json::to_value(req)
                    .ok()
                    .and_then(|v| v.get("bearer_auth").cloned())
                    .is_some()
            })
        });
        assert!(has_bearer, "{path} must require bearer_auth in OpenAPI");

        assert!(op.responses.responses.contains_key("200"));
        assert!(op.responses.responses.contains_key("400"));
        assert!(op.responses.responses.contains_key("401"));
    }
}

async fn disposable_db_state() -> AppState {
    let url = std::env::var("SECURITY_TEST_DATABASE_URL").expect("disposable PostgreSQL required");
    assert!(url.ends_with("/toss_security_tests"));
    let db = infra::db::connect(&url).await.unwrap();
    let config = Config {
        port: 0,
        log_level: "error".into(),
        node_env: "test".into(),
        database_url: url,
        redis_url: String::new(),
        bullmq_prefix: "activity-test".into(),
        bullmq_concurrency: 1,
        supabase_url: "http://127.0.0.1:9".into(),
        supabase_auth_api_key: "test".into(),
        github_app_id: String::new(),
        github_app_private_key: String::new(),
        github_webhook_secret: String::new(),
        platform_stellar_public_key: String::new(),
        platform_stellar_secret_key: String::new(),
        dispute_resolver_stellar_public_key: String::new(),
        dispute_resolver_stellar_secret_key: String::new(),
        trustless_work_api_key: String::new(),
        trustless_work_base_url: String::new(),
        trustless_work_health_escrow_contract: String::new(),
        token_address: String::new(),
        stellar_network: "testnet".into(),
        app_url: String::new(),
        webhook_url: None,
        dev_webhook_proxy_enabled: false,
        auth_bypass: false,
        smee_source_url: String::new(),
        smee_target_url: String::new(),
        resend_api_key: String::new(),
        email_from: String::new(),
        aws_access_key_id: String::new(),
        aws_secret_access_key: String::new(),
        aws_region: "us-east-1".into(),
        s3_endpoint_url: None,
        s3_bucket_name: String::new(),
    };
    AppState {
        config: Arc::new(config),
        db,
        redis: None,
        cache: infra::redis::Cache::new(None),
        queue: infra::queue::QueueInfra::disabled(),
        http_client: reqwest::Client::new(),
        email: resend_rs::Resend::new("test"),
        s3_client: aws_sdk_s3::Client::from_conf(
            aws_sdk_s3::config::Builder::new()
                .behavior_version_latest()
                .region(aws_sdk_s3::config::Region::new("us-east-1"))
                .build(),
        ),
    }
}

#[tokio::test]
#[ignore = "requires SECURITY_TEST_DATABASE_URL pointing to disposable toss_security_tests"]
async fn db_activity_and_leaderboard_enforce_auth_and_maintainer_isolation() {
    let state = disposable_db_state().await;
    let mut db = state.db.clone();
    let tag = Uuid::new_v4();
    let base_id = (tag.as_u128() & (i64::MAX as u128 - 10)) as i64;

    let m1 = schema::Profile::create()
        .github_id(base_id + 1)
        .username(format!("maint-a-{}", tag.simple()))
        .exec(&mut db)
        .await
        .unwrap();
    let m2 = schema::Profile::create()
        .github_id(base_id + 2)
        .username(format!("maint-b-{}", tag.simple()))
        .exec(&mut db)
        .await
        .unwrap();

    let repo_a = schema::Repository::create()
        .github_repo_id(base_id + 3)
        .full_name(format!("owner-a/repo-{}", tag.simple()))
        .exec(&mut db)
        .await
        .unwrap();
    let repo_b = schema::Repository::create()
        .github_repo_id(base_id + 4)
        .full_name(format!("owner-b/repo-{}", tag.simple()))
        .exec(&mut db)
        .await
        .unwrap();

    schema::RepoMaintainer::create()
        .repo_id(repo_a.id)
        .profile_id(m1.id)
        .exec(&mut db)
        .await
        .unwrap();
    schema::RepoMaintainer::create()
        .repo_id(repo_b.id)
        .profile_id(m2.id)
        .exec(&mut db)
        .await
        .unwrap();

    schema::Activity::create()
        .repo_id(Some(repo_a.id))
        .event_type("locked".to_string())
        .payload(Some(json!({
            "issue_number": null,
            "issue_title": null,
            "amount_usdc": "750",
            "tx_hash": format!("pool-tx-{}", tag.simple())
        })))
        .exec(&mut db)
        .await
        .unwrap();

    schema::Activity::create()
        .repo_id(Some(repo_b.id))
        .event_type("released".to_string())
        .payload(Some(json!({
            "issue_number": 5,
            "issue_title": "Secret B",
            "amount_usdc": "999",
            "contributor": "secret_dev",
            "tx_hash": format!("secret-tx-{}", tag.simple())
        })))
        .exec(&mut db)
        .await
        .unwrap();

    // Maintainer A only sees repo_a's pool deposit, never repo_b's released event
    let batches = load_maintainer_event_batches(&state, m1.github_id, None)
        .await
        .unwrap();
    let events = flatten_batches(&batches);
    let feed = query_activity(events, &ActivityQuery::default().validate().unwrap());
    assert_eq!(feed.total, 1);
    assert_eq!(feed.data[0].repo, repo_a.full_name);
    assert_eq!(feed.data[0].kind, ActivityEventKind::Locked);
    assert_eq!(feed.data[0].issue_number, None);
    assert_eq!(feed.data[0].issue_title, None);

    // Real router enforces 401 when unauthenticated
    let app = toss_backend::routes::router().with_state(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let resp = reqwest::Client::new()
        .get(format!("http://{addr}/api/activity"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    server.abort();
}
