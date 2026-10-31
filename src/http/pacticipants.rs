use crate::error::{Error, Result};
use crate::http::hal::Hal;
use axum::extract::Path;

pub async fn get_pacticipant(Path(name): Path<String>) -> Result<Hal> {
    Err(Error::NotFound(format!(
        "No pacticipant with name '{name}' found"
    )))
}
