pub const REPO_TTL: u64 = 300;
pub const GH_TOKEN_TTL: u64 = 3000;
pub const CONTRIB_TTL: u64 = 300;

pub fn repo(id: uuid::Uuid) -> String {
    format!("repo:{id}")
}

pub fn repo_by_github_id(github_repo_id: i64) -> String {
    format!("repo:gh:{github_repo_id}")
}

pub fn gh_token(github_repo_id: i64) -> String {
    format!("gh-token:{github_repo_id}")
}

pub fn contrib(github_user_id: i64) -> String {
    format!("contrib:{github_user_id}")
}
