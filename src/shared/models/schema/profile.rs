use jiff::Timestamp;
use uuid::Uuid;

#[derive(toasty::Model)]
#[table = "profile"]
pub struct Profile {
    #[key]
    #[auto]
    pub id: Uuid,

    #[unique]
    #[index]
    pub github_id: Uuid,

    #[unique]
    #[index]
    pub username: String,

    full_name: String,

    email: String,

    avatar_url: Option<String>,

    bio: Option<String>,

    location: Option<String>,

    skills: Option<Vec<String>>,

    telegram: Option<String>,

    discord: Option<String>,

    twitter: Option<String>,

    #[default(Timestamp::now())]
    created_at: Timestamp,

    #[default(Timestamp::now())]
    updated_at: Timestamp,
}
