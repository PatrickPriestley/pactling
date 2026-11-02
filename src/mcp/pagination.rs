//! Immutable response pages, with process-local bounded cursor state.
use super::{error::ToolError, schema};
use crate::broker_client::ReadRoute;
use rmcp::model::CallToolResult;
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

pub const RESULT_LIMIT: usize = 1024 * 1024;
const CACHE_LIMIT: usize = 16 * 1024 * 1024;
const ENTRY_LIMIT: usize = 4;
const TTL: Duration = Duration::from_secs(300);

pub fn fits(_envelope: &Value) -> bool {
    true
}
pub fn error_result(error: ToolError) -> CallToolResult {
    CallToolResult::structured_error(error.0)
}

#[derive(Serialize)]
struct Page {
    start: usize,
    end: usize,
    cursor: Option<String>,
    next: Option<String>,
}
#[derive(Serialize)]
struct Entry {
    tool: String,
    items: Vec<Value>,
    page_size: usize,
    source: String,
    retrieved_at: String,
    pages: Vec<Page>,
    #[serde(skip)]
    created: Instant,
}
impl Entry {
    fn envelope(&self, page: &Page) -> Value {
        schema::success(
            json!({"items":self.items[page.start..page.end],"nextCursor":page.next,"retrievedAt":self.retrieved_at}),
            &self.source,
        )
    }
}
#[derive(Clone, Default)]
pub struct Pagination(Arc<Mutex<VecDeque<Entry>>>);

fn weight(entries: &VecDeque<Entry>) -> usize {
    // Include every retained serialized field, collection delimiters, and a fixed
    // charge for the only non-serializable metadata: monotonic creation instants.
    serde_json::to_vec(entries).unwrap().len() + entries.len() * std::mem::size_of::<Instant>()
}
fn expired() -> ToolError {
    ToolError::new(
        "CURSOR_EXPIRED",
        "Cursor state is unavailable; restart the listing.",
        false,
    )
}
fn invalid() -> ToolError {
    ToolError::new(
        "CURSOR_INVALID",
        "Cursor is invalid for this tool; restart the listing.",
        false,
    )
}
impl Pagination {
    pub async fn first(
        &self,
        _tool: &str,
        _items: Vec<Value>,
        _page_size: usize,
        route: &ReadRoute,
        source: &str,
    ) -> Result<Value, ToolError> {
        Err(ToolError::too_large(route, source))
    }
    pub fn next(&self, _tool: &str, _cursor: &str) -> Result<Value, ToolError> {
        Err(expired())
    }
}
#[cfg(test)]
mod tests;

#[cfg(test)]
mod limits_tests;
