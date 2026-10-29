use jiff::Timestamp;
use uuid::Uuid;

use super::{repositories::Repository, Profile};

#[derive(Debug, toasty::Model)]
#[table = "repo_maintainers"]
pub struct RepoMaintainer {
    #[key]
    #[index]
    pub repo_id: Uuid,
    #[belongs_to(key = repo_id, references = id)]
    pub repo: toasty::Deferred<Repository>,

    #[key]
    #[index]
    pub profile_id: Uuid,
    #[belongs_to(key = profile_id, references = id)]
    pub profile: toasty::Deferred<Profile>,

    #[default(String::from("maintainer"))]
    pub role: String,

    #[default(Timestamp::now())]
    pub added_at: Timestamp,
}
