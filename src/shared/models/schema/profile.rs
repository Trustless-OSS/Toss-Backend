use jiff::Timestamp;
use uuid::Uuid;

#[derive(Debug, toasty::Model)]
#[table = "profiles"]
pub struct Profile {
    #[key]
    #[auto]
    pub id: Uuid,

    #[unique]
    #[index]
    pub github_id: i64,

    #[unique]
    #[index]
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
}
