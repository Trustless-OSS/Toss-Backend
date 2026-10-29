use uuid::Uuid;

pub const REPO_TTL: u64 = 300;
pub const GH_TOKEN_TTL: u64 = 3000;
pub const CONTRIB_TTL: u64 = 300;
pub const NOTIFICATION_TTL: u64 = 300;

pub fn repo(id: Uuid) -> String {
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

pub fn notify(id: Uuid) -> String {
    format!("notification:{id}")
}

pub fn notification_unread_count(profile_id: Uuid) -> String {
    format!("notification:unread-count:{profile_id}")
}

pub fn notification_feed_version(profile_id: Uuid) -> String {
    format!("notification:feed-version:{profile_id}")
}

pub fn notification_feed(
    profile_id: Uuid,
    version: &str,
    unread_only: bool,
    limit: i64,
    offset: i64,
) -> String {
    format!(
        "notification:feed:{profile_id}:{version}:unread={unread_only}:limit={limit}:offset={offset}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notification_feed_keys_are_unique_per_page_and_revision() {
        let profile_id = Uuid::nil();
        let first_page = notification_feed(profile_id, "revision-a", false, 20, 0);
        let second_page = notification_feed(profile_id, "revision-a", false, 20, 20);
        let refreshed_page = notification_feed(profile_id, "revision-b", false, 20, 0);

        assert_ne!(first_page, second_page);
        assert_ne!(first_page, refreshed_page);
    }
}
