use uuid::Uuid;
use jiff::Timestamp;




#[derive(Debug , toasty::Model)]
#[table="repo_maintainers"]
pub struct RepoMaintainers {
         #[key]
         pub repo_id: Uuid , 

         #[index]
         pub profile_id : Uuid , 
         
         #[default("maintainer".to_string())]
         pub role : String,
         
         #[default(Timestamp::now())]
         pub created_at : Timestamp


}