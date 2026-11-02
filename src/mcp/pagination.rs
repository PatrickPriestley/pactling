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

#[cfg(test)]
thread_local! {
    pub(super) static FITS_CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}
pub fn fits(envelope: &Value) -> bool {
    #[cfg(test)]
    FITS_CALLS.with(|c| c.set(c.get() + 1));
    serde_json::to_vec(&CallToolResult::structured(envelope.clone()))
        .unwrap()
        .len()
        <= RESULT_LIMIT
}

/// All error fields are fixed and small except the optional, encoded source URL.
pub fn error_result(mut error: ToolError) -> CallToolResult {
    let result = CallToolResult::structured_error(error.0.clone());
    if serde_json::to_vec(&result).unwrap().len() <= RESULT_LIMIT {
        return result;
    }
    error.0["error"]
        .as_object_mut()
        .unwrap()
        .remove("evidenceUrl");
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
        tool: &str,
        items: Vec<Value>,
        page_size: usize,
        route: &ReadRoute,
        source: &str,
    ) -> Result<Value, ToolError> {
        let mut entry = Entry {
            tool: tool.to_owned(),
            items,
            page_size,
            source: source.to_owned(),
            retrieved_at: chrono::Utc::now().to_rfc3339(),
            pages: vec![],
            created: Instant::now(),
        };
        let mut start = 0;
        let mut cursor = None;
        loop {
            // Preparation owns no shared cache state and can be dropped on cancellation.
            tokio::task::yield_now().await;
            let len = entry.items.len();
            let max_end = (start + entry.page_size).min(len);
            let next_token = uuid::Uuid::new_v4().to_string();
            let mut page = Page {
                start,
                end: max_end,
                cursor,
                next: None,
            };
            let set_end = |page: &mut Page, end: usize| {
                page.end = end;
                page.next = (end < len).then(|| next_token.clone());
            };
            set_end(&mut page, max_end);
            if !fits(&entry.envelope(&page)) {
                // A page grows with each added item, so binary search for the largest fit:
                // `fit` always fits (or is `start`, no items) and `too_big` never does.
                let (mut fit, mut too_big) = (start, max_end);
                while too_big - fit > 1 {
                    let mid = fit + (too_big - fit) / 2;
                    set_end(&mut page, mid);
                    if fits(&entry.envelope(&page)) {
                        fit = mid;
                    } else {
                        too_big = mid;
                    }
                    tokio::task::yield_now().await;
                }
                if fit == start {
                    return Err(ToolError::too_large(route, source));
                }
                set_end(&mut page, fit);
            }
            start = page.end;
            cursor = page.next.clone();
            entry.pages.push(page);
            if start == entry.items.len() {
                break;
            }
        }
        let first = entry.envelope(&entry.pages[0]);
        let mut incoming = VecDeque::from([entry]);
        if weight(&incoming) > CACHE_LIMIT {
            return Err(ToolError::too_large(route, source));
        }
        let mut entries = self.0.lock().unwrap();
        entries.retain(|e| e.created.elapsed() < TTL);
        entries.append(&mut incoming);
        while entries.len() > ENTRY_LIMIT || weight(&entries) > CACHE_LIMIT {
            entries.pop_front();
        }
        Ok(first)
    }
    pub fn next(&self, tool: &str, cursor: &str) -> Result<Value, ToolError> {
        uuid::Uuid::parse_str(cursor).map_err(|_| invalid())?;
        let mut entries = self.0.lock().unwrap();
        entries.retain(|e| e.created.elapsed() < TTL);
        for entry in entries.iter() {
            if let Some(page) = entry
                .pages
                .iter()
                .find(|p| p.cursor.as_deref() == Some(cursor))
            {
                if entry.tool != tool {
                    return Err(invalid());
                }
                return Ok(entry.envelope(page));
            }
        }
        Err(expired())
    }
}
#[cfg(test)]
mod limits_tests;
#[cfg(test)]
mod tests;
