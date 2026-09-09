use crate::error::AppError;
use resend_rs::Resend;

pub async fn connect(api_key: &str) -> Result<Resend, AppError> {
    if api_key.trim().is_empty() {
        return Err(AppError::internal("Empty Resend API key"));
    }

    Ok(Resend::new(api_key))
}
