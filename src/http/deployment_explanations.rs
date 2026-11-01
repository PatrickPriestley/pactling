//! Versioned deployment explanation projection (implemented after contract tests).
use crate::http::hal::Links;
use crate::service::matrix::evaluation::DecisionEvaluation;
use serde_json::Value;

pub fn document(_links: &Links, _evaluation: &DecisionEvaluation, _query: &str) -> Value {
    Value::Null
}
