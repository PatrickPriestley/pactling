//! Immutable response pages, with process-local bounded cursor state.
use super::error::ToolError;
use crate::broker_client::ReadRoute;
use serde_json::Value;

#[derive(Clone, Default)]
pub struct Pagination;
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
        Err(ToolError::new(
            "CURSOR_EXPIRED",
            "Cursor state is unavailable; restart the listing.",
            false,
        ))
    }
}
#[cfg(test)]
mod tests;
