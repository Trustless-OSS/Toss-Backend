use jiff::Timestamp;
use uuid::Uuid;

// composite PK: (repo_id, profile_id)
#[derive(Debug, toasty::Model)]
#[table = "repo_maintainers"]
pub struct RepoMaintainer {
    #[key]
    pub repo_id: Uuid,

    #[key]
    #[index]
    pub profile_id: Uuid,

    // role: 'owner' | 'maintainer'
    #[default(String::from("maintainer"))]
    pub role: String,

    #[default(Timestamp::now())]
    pub added_at: Timestamp,
}
