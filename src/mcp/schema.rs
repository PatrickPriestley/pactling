//! The checked-in schemas are discovery and runtime validation's single source of truth.
use serde_json::{Value, json};
use std::sync::LazyLock;

pub struct Contract {
    pub name: &'static str,
    pub input: Value,
    pub output: Value,
    pub input_validator: jsonschema::Validator,
    pub output_validator: jsonschema::Validator,
}
macro_rules! contract {
    ($name:literal) => {{
        let input: Value = serde_json::from_str(include_str!(concat!(
            "../../docs/api/mcp/",
            $name,
            ".input.schema.json"
        )))
        .expect("checked input schema");
        let output: Value = serde_json::from_str(include_str!(concat!(
            "../../docs/api/mcp/",
            $name,
            ".output.schema.json"
        )))
        .expect("checked output schema");
        Contract {
            name: $name,
            input_validator: jsonschema::validator_for(&input).expect("valid bundled input schema"),
            output_validator: jsonschema::validator_for(&output)
                .expect("valid bundled output schema"),
            input,
            output,
        }
    }};
}
pub static CONTRACTS: LazyLock<[Contract; 5]> = LazyLock::new(|| {
    [
        contract!("pactling_explain_deployment"),
        contract!("pactling_get_pact"),
        contract!("pactling_list_integrations"),
        contract!("pactling_list_pacticipants"),
        contract!("pactling_matrix"),
    ]
});

impl Contract {
    pub fn invalid_field(&self, arguments: &Value) -> Option<&'static str> {
        // Precedence is deliberate, even when either field has the wrong type.
        if arguments.get("cursor").is_some() && arguments.get("pageSize").is_some() {
            return Some("/cursor");
        }
        let error = self.input_validator.iter_errors(arguments).next()?;
        // Never include user-supplied unknown keys, values, or validator diagnostics.
        let path = error.instance_path().to_string();
        Some(match path.split('/').nth(1) {
            Some("consumer") => "/consumer",
            Some("provider") => "/provider",
            Some("consumerVersion") => "/consumerVersion",
            Some("participantA") => "/participantA",
            Some("participantB") => "/participantB",
            Some("latestby") => "/latestby",
            Some("pacticipant") => "/pacticipant",
            Some("selector") => "/selector",
            Some("target") => "/target",
            Some("ignores") => "/ignores",
            Some("pageSize") => "/pageSize",
            Some("cursor") => "/cursor",
            _ => "",
        })
    }
}
pub fn success(data: Value, source: &str) -> Value {
    json!({"adapterSchemaVersion":"1.0", "data":data, "coverage":"recorded_only", "evidenceLinks":[{"rel":"source","href":source}]})
}
