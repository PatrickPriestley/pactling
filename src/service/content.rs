//! Pact content identity, matching the Ruby broker's hashes exactly (verified by
//! recomputing captured values; see docs/plans Task 2.1).
//!
//! * An interaction's `_id` is the SHA-1 of the interaction serialised as compact JSON
//!   with object keys sorted recursively, with any existing `_id` removed.
//! * A pact version's sha is the SHA-1 of compact, key-sorted JSON of the content that
//!   affects verification: `interactions` and `messages` (each sorted by their compact
//!   key-sorted JSON, without `_id`) and `pact_specification_version`, keeping only the
//!   keys that are present. A pact with neither array hashes its whole sorted content.
use serde_json::{Map, Value};
use sha1::{Digest, Sha1};

fn sha1_hex(s: &str) -> String {
    Sha1::digest(s.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The value with every object's keys sorted, recursively.
fn sorted(v: &Value) -> Value {
    match v {
        Value::Object(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            Value::Object(
                keys.into_iter()
                    .map(|k| (k.clone(), sorted(&m[k])))
                    .collect(),
            )
        }
        Value::Array(a) => Value::Array(a.iter().map(sorted).collect()),
        other => other.clone(),
    }
}

/// Compact JSON with sorted keys: what Ruby's `to_json` on a key-sorted hash yields.
fn canonical(v: &Value) -> String {
    serde_json::to_string(&sorted(v)).expect("json")
}

fn without_id(interaction: &Value) -> Value {
    match interaction {
        Value::Object(m) => Value::Object(
            m.iter()
                .filter(|(k, _)| k.as_str() != "_id")
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        ),
        other => other.clone(),
    }
}

pub fn interaction_id(interaction: &Value) -> String {
    sha1_hex(&canonical(&without_id(interaction)))
}

/// `metadata.pactSpecification.version`, `metadata.pact-specification.version` or
/// `metadata.pactSpecificationVersion` (the last two assumed from the Ruby source).
pub fn pact_specification_version(pact: &Value) -> Option<String> {
    let metadata = pact.get("metadata")?;
    metadata
        .get("pactSpecification")
        .and_then(|p| p.get("version"))
        .or_else(|| {
            metadata
                .get("pact-specification")
                .and_then(|p| p.get("version"))
        })
        .or_else(|| metadata.get("pactSpecificationVersion"))
        .and_then(|v| v.as_str())
        .map(str::to_owned)
}

pub fn pact_version_sha(pact: &Value) -> String {
    let mut content = Map::new();
    for key in ["interactions", "messages"] {
        if let Some(Value::Array(items)) = pact.get(key) {
            let mut sorted_items: Vec<String> =
                items.iter().map(|i| canonical(&without_id(i))).collect();
            sorted_items.sort();
            // Re-parse so the array serialises as JSON values, not strings.
            let values: Vec<Value> = sorted_items
                .iter()
                .map(|s| serde_json::from_str(s).expect("round trip"))
                .collect();
            content.insert(key.to_owned(), Value::Array(values));
        }
    }
    if content.is_empty() {
        return sha1_hex(&canonical(pact));
    }
    if let Some(v) = pact_specification_version(pact) {
        content.insert("pact_specification_version".to_owned(), Value::String(v));
    }
    sha1_hex(&canonical(&Value::Object(content)))
}

/// The pact with `_id` set on every interaction and message, as the broker serves it.
pub fn with_ids(mut pact: Value) -> Value {
    for key in ["interactions", "messages"] {
        if let Some(Value::Array(items)) = pact.get_mut(key) {
            for item in items.iter_mut() {
                let id = interaction_id(item);
                if let Value::Object(m) = item {
                    let mut with: Map<String, Value> = Map::new();
                    with.insert("_id".to_owned(), Value::String(id));
                    for (k, v) in m.iter().filter(|(k, _)| k.as_str() != "_id") {
                        with.insert(k.clone(), v.clone());
                    }
                    *m = with;
                }
            }
        }
    }
    pact
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture() -> Value {
        serde_json::from_str(include_str!(
            "../../tests/fixtures/pact/consumer-provider.json"
        ))
        .unwrap()
    }

    #[test]
    fn interaction_ids_match_ruby_broker() {
        let pact = fixture();
        let ids: Vec<String> = pact["interactions"]
            .as_array()
            .unwrap()
            .iter()
            .map(interaction_id)
            .collect();
        assert_eq!(
            ids,
            [
                "72e9b4bb90f5cb675f1cea3e426b11616e6703a0",
                "8da86afba68368b3a91327afde5a9c6b59c02274"
            ]
        );
    }

    #[test]
    fn interaction_id_ignores_an_existing_id_and_key_order() {
        let a = json!({"description": "d", "request": {"path": "/x", "method": "GET"}});
        let b =
            json!({"_id": "stale", "request": {"method": "GET", "path": "/x"}, "description": "d"});
        assert_eq!(interaction_id(&a), interaction_id(&b));
    }

    #[test]
    fn pact_version_sha_matches_ruby_broker() {
        assert_eq!(
            pact_version_sha(&fixture()),
            "0350e2d94a510324ee31cd7375bf1660e5d8c485"
        );
        assert_eq!(
            pact_version_sha(
                &json!({"consumer": {"name": "Consumer"}, "provider": {"name": "Provider"}, "interactions": [], "metadata": {}})
            ),
            "ed7c1aebc3e11064ce9afc29297e1e36203583cd",
            "names and empty metadata do not affect the sha"
        );
        assert_eq!(
            pact_version_sha(&json!({"interactions": []})),
            "ed7c1aebc3e11064ce9afc29297e1e36203583cd"
        );
    }

    #[test]
    fn pact_version_sha_orders_interactions_by_their_sorted_json() {
        // Creation order, description order and JSON order all differ here; the Ruby
        // broker produced 1d9541ef... for this pact (fixture PUT_pact_7.0.0_three_interactions).
        let pact = json!({
            "consumer": {"name": "Consumer"}, "provider": {"name": "Provider"},
            "interactions": [
                {"description": "zeta", "request": {"method": "GET", "path": "/a"}, "response": {"status": 200}},
                {"description": "alpha", "providerState": "zz", "request": {"method": "GET", "path": "/b"}, "response": {"status": 200}},
                {"description": "mid", "request": {"method": "POST", "path": "/c"}, "response": {"status": 201}}
            ],
            "metadata": {"pactSpecification": {"version": "2.0.0"}}
        });
        assert_eq!(
            pact_version_sha(&pact),
            "1d9541ef143e4acb6bad03910cd7cf2864b71473"
        );
    }

    #[test]
    fn pact_version_sha_ignores_ids_already_present() {
        let plain = fixture();
        let with = with_ids(plain.clone());
        assert_eq!(
            with["interactions"][0]["_id"],
            "72e9b4bb90f5cb675f1cea3e426b11616e6703a0"
        );
        assert_eq!(pact_version_sha(&with), pact_version_sha(&plain));
    }

    #[test]
    fn with_ids_puts_id_first_and_keeps_the_rest() {
        let with = with_ids(fixture());
        let first = with["interactions"][0].as_object().unwrap();
        assert_eq!(first.keys().next().unwrap(), "_id");
        assert_eq!(first["description"], "a request for an order");
        assert_eq!(with["consumer"]["name"], "Consumer");
    }

    #[test]
    fn with_ids_handles_messages_and_pacts_without_interactions() {
        let msg = json!({"messages": [{"description": "m", "contents": {"a": 1}}]});
        let with = with_ids(msg.clone());
        assert_eq!(
            with["messages"][0]["_id"],
            interaction_id(&msg["messages"][0])
        );
        assert_eq!(with_ids(json!({"foo": 1})), json!({"foo": 1}));
    }

    #[test]
    fn pact_without_interactions_or_messages_hashes_its_whole_content() {
        let a = json!({"b": 1, "a": 2});
        let b = json!({"a": 2, "b": 1});
        assert_eq!(pact_version_sha(&a), pact_version_sha(&b));
        assert_ne!(
            pact_version_sha(&a),
            pact_version_sha(&json!({"a": 3, "b": 1}))
        );
    }

    #[test]
    fn specification_version_is_read_from_the_three_metadata_shapes() {
        assert_eq!(
            pact_specification_version(
                &json!({"metadata": {"pactSpecification": {"version": "3.0.0"}}})
            )
            .as_deref(),
            Some("3.0.0")
        );
        assert_eq!(
            pact_specification_version(
                &json!({"metadata": {"pact-specification": {"version": "2.0.0"}}})
            )
            .as_deref(),
            Some("2.0.0")
        );
        assert_eq!(
            pact_specification_version(&json!({"metadata": {"pactSpecificationVersion": "1.0.0"}}))
                .as_deref(),
            Some("1.0.0")
        );
        assert_eq!(pact_specification_version(&json!({"metadata": {}})), None);
        assert_eq!(pact_specification_version(&json!({})), None);
    }
}
