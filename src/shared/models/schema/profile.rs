use super::{Activity, Bounty, EscrowFunder, Notification, RepoMaintainer, Wallet};
use jiff::Timestamp;
use uuid::Uuid;

#[derive(Debug, toasty::Model)]
#[table = "profiles"]
pub struct Profile {
    #[key]
    #[auto]
    pub id: Uuid,

    #[unique]
    pub github_id: i64,

    #[unique]
    pub username: String,

    pub full_name: Option<String>,
    pub email: Option<String>,
    pub avatar_url: Option<String>,
    pub bio: Option<String>,
    pub location: Option<String>,
    pub skills: Option<Vec<String>>,
    pub telegram: Option<String>,
    pub discord: Option<String>,
    pub twitter: Option<String>,

    #[default(Timestamp::now())]
    pub created_at: Timestamp,

    #[default(Timestamp::now())]
    pub updated_at: Timestamp,

    #[has_many(pair = profile)]
    pub wallets: toasty::Deferred<Vec<Wallet>>,

    #[has_many(pair = profile)]
    pub notifications: toasty::Deferred<Vec<Notification>>,

    #[has_many(pair = profile)]
    pub maintained_repos: toasty::Deferred<Vec<RepoMaintainer>>,

    #[has_many(pair = profile)]
    pub funded_escrows: toasty::Deferred<Vec<EscrowFunder>>,

    #[has_many(pair = assignee)]
    pub assigned_bounties: toasty::Deferred<Vec<Bounty>>,

    #[has_many(pair = actor)]
    pub activity: toasty::Deferred<Vec<Activity>>,
}
