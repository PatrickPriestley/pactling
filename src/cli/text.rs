//! Concise, one-line-per-reason presentation of an already evaluated explanation.
use serde_json::Value;

fn clean(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn scalar(value: &Value) -> Option<String> {
    value.as_str().map(clean)
}

fn identity(name: &str, version: &Value) -> String {
    format!(
        "{}@{}",
        clean(name),
        scalar(version).unwrap_or_else(|| "?".into())
    )
}

fn pair(value: &Value) -> Option<String> {
    Some(format!(
        "{} -> {}",
        identity(value["consumer"].as_str()?, &value["consumerVersion"]),
        identity(value["provider"].as_str()?, &value["providerVersion"])
    ))
}

fn subject(value: &Value) -> Option<String> {
    let selector = value.get("requested").unwrap_or(value);
    let name = selector["pacticipant"].as_str()?;
    let version = scalar(&selector["version"]);
    Some(match version {
        Some(version) => format!("{}@{version}", clean(name)),
        None => clean(name),
    })
}

fn push_unique(values: &mut Vec<String>, value: String) {
    if !values.contains(&value) {
        values.push(value);
    }
}

/// Reason pointers refer to a row or one of its fields, never similarly prefixed rows.
fn referenced_row<'a>(document: &'a Value, reference: &str) -> Option<&'a Value> {
    if let Some(tail) = reference.strip_prefix("/rows/") {
        let index = tail.split('/').next()?.parse::<usize>().ok()?;
        return document["rows"].get(index);
    }
    let tail = reference.strip_prefix("/resolution/excluded/")?;
    let index = tail.split('/').next()?.parse::<usize>().ok()?;
    document["resolution"]["excluded"]
        .get(index)?
        .get("evidence")
}

fn linked_actions<'a>(document: &'a Value, reason: &Value) -> Vec<&'a Value> {
    reason["actionIds"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|id| {
            let id = id.as_str()?;
            document["nextActions"]
                .as_array()?
                .iter()
                .find(|action| action["id"].as_str() == Some(id))
        })
        .collect()
}

fn context(document: &Value, reason: &Value, actions: &[&Value]) -> String {
    let mut subjects = vec![];
    // Action targets preserve exact provider candidates when Ruby's row version is null.
    for action in actions {
        if let Some(label) = pair(&action["target"]).or_else(|| subject(&action["target"])) {
            push_unique(&mut subjects, label);
        }
    }
    if subjects.is_empty() {
        for reference in reason["evidenceRefs"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            let label = referenced_row(document, reference).and_then(pair);
            if let Some(label) = label {
                push_unique(&mut subjects, label);
            }
        }
    }
    if subjects.is_empty() {
        for reference in reason["evidenceRefs"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            if let Some(label) = document.pointer(reference).and_then(subject) {
                push_unique(&mut subjects, label);
            }
        }
    }
    if subjects.is_empty() {
        "no recorded pair".into()
    } else {
        subjects.join(", ")
    }
}

fn action_summary(document: &Value, action: &Value) -> String {
    let target = &action["target"];
    let kind = scalar(&action["type"]).unwrap_or_else(|| "unknown action".into());
    let label = target["provider"]
        .as_str()
        .map(|name| identity(name, &target["providerVersion"]))
        .or_else(|| subject(target))
        .or_else(|| scalar(&action["message"]))
        .unwrap_or_else(|| "unspecified target".into());
    let mut details = vec![];
    for (key, caption) in [
        ("contractSha", "contract"),
        ("environment", "environment"),
        ("tag", "tag"),
    ] {
        if let Some(value) = scalar(&target[key]) {
            push_unique(&mut details, format!("{caption} {value}"));
        }
    }
    let requested = &target["requested"];
    for (key, caption) in [("branch", "branch"), ("tag", "tag")] {
        if let Some(value) = scalar(&requested[key]) {
            push_unique(&mut details, format!("{caption} {value}"));
        }
    }
    if requested["mainBranch"] == true {
        push_unique(&mut details, "main branch".into());
    }
    if requested["latest"] == true {
        push_unique(&mut details, "latest".into());
    }
    let href = scalar(&target["verificationHref"]).or_else(|| {
        action["evidenceRefs"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .find_map(|reference| document.pointer(reference).and_then(|e| scalar(&e["href"])))
    });
    if let Some(href) = href {
        push_unique(&mut details, href);
    }
    if details.is_empty() {
        format!("{kind} {label}")
    } else {
        format!("{kind} {label} ({})", details.join("; "))
    }
}

pub(super) fn render(document: &Value) -> String {
    let mut out = format!(
        "Decision: {}\n",
        scalar(&document["decision"]).unwrap_or_else(|| "unknown".into())
    );
    for reason in document["reasons"].as_array().into_iter().flatten() {
        let actions = linked_actions(document, reason);
        let summaries: Vec<_> = actions
            .iter()
            .map(|action| action_summary(document, action))
            .collect();
        let action = if summaries.is_empty() {
            "no action".into()
        } else {
            summaries.join("; ")
        };
        out.push_str(&format!(
            "[{}] {}: {action}\n",
            scalar(&reason["code"]).unwrap_or_else(|| "unknown reason".into()),
            context(document, reason, &actions)
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn example(name: &str) -> Value {
        let path = format!(
            "{}/tests/fixtures/pactling/explanations/{name}.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn fixtures_become_one_line_per_reason_with_exact_targets_and_links() {
        for (name, code, action) in [
            (
                "allowed",
                "ALL_REQUIRED_VERIFICATIONS_SUCCESSFUL",
                "no action",
            ),
            ("blocked", "VERIFICATION_FAILED", "FIX_FAILED_VERIFICATION"),
            ("unknown", "VERIFICATION_MISSING", "VERIFY_CONTRACT"),
        ] {
            let doc = example(name);
            let out = render(&doc);
            assert_eq!(out.lines().count(), 2, "{out}");
            assert_eq!(out.lines().next().unwrap(), format!("Decision: {name}"));
            assert!(
                out.contains(&format!("[{code}] Checkout@1 -> Payments@2: {action}")),
                "{out}"
            );
            assert!(
                !out.contains("Target:") && !out.contains("Evidence ") && !out.contains('{'),
                "{out}"
            );
            if name != "allowed" {
                assert!(
                    out.contains(
                        doc["nextActions"][0]["target"]["contractSha"]
                            .as_str()
                            .unwrap()
                    )
                );
                assert!(out.contains(doc["rows"][0]["contract"]["href"].as_str().unwrap()));
                assert_eq!(
                    out.matches(doc["rows"][0]["contract"]["href"].as_str().unwrap())
                        .count(),
                    1,
                    "only the most useful link: {out}"
                );
            }
            if name == "blocked" {
                assert!(out.contains(doc["rows"][0]["verification"]["href"].as_str().unwrap()));
            }
        }
    }

    #[test]
    fn every_role_and_linked_action_is_reported_without_unrelated_actions() {
        let mut doc = example("unknown");
        doc["nextActions"][0]["type"] = json!("FUTURE_ACTION");
        let mut second = doc["nextActions"][0].clone();
        second["id"] = json!("action-2");
        second["target"]["provider"] = json!("Ledger");
        second["target"]["providerVersion"] = json!("3");
        doc["nextActions"].as_array_mut().unwrap().push(second);
        let mut unrelated = doc["nextActions"][0].clone();
        unrelated["id"] = json!("unrelated");
        unrelated["type"] = json!("UNRELATED_ACTION");
        doc["nextActions"].as_array_mut().unwrap().push(unrelated);
        doc["reasons"][0]["code"] = json!("FUTURE_REASON");
        doc["reasons"][0]["actionIds"] = json!(["action-1", "action-2"]);
        doc["reasons"].as_array_mut().unwrap().push(json!({"code":"ROW_IGNORED","role":"advisory","evidenceRefs":["/rows/0/verification"],"actionIds":[]}));
        doc["reasons"].as_array_mut().unwrap().push(
            json!({"code":"SUPPORTING","role":"supporting","evidenceRefs":[],"actionIds":[]}),
        );
        let out = render(&doc);
        assert_eq!(out.lines().count(), 4, "{out}");
        assert!(
            out.contains(
                "[FUTURE_REASON] Checkout@1 -> Payments@2, Checkout@1 -> Ledger@3: FUTURE_ACTION"
            ),
            "{out}"
        );
        assert!(
            out.contains("[ROW_IGNORED] Checkout@1 -> Payments@?: no action"),
            "{out}"
        );
        assert!(
            out.contains("[SUPPORTING] no recorded pair: no action"),
            "{out}"
        );
        assert!(!out.contains("UNRELATED_ACTION"), "{out}");
    }

    #[test]
    fn selectors_and_ignores_have_subjects_without_invented_pairs() {
        let doc = json!({"decision":"blocked","rows":[],"reasons":[
            {"code":"SELECTOR_VERSION_MISSING","actionIds":["a"],"evidenceRefs":[]},
            {"code":"IGNORE_TARGET_NOT_FOUND","actionIds":["b"],"evidenceRefs":[]}
        ],"nextActions":[
            {"id":"a","type":"RESOLVE_SELECTOR","target":{"selectorIndex":0,"requested":{"pacticipant":"C","version":"missing","latest":false,"tag":null,"branch":null,"mainBranch":false}},"evidenceRefs":[]},
            {"id":"b","type":"CORRECT_IGNORE_REQUEST","target":{"ignoreIndex":0,"pacticipant":"P","version":"9"},"evidenceRefs":[]}
        ]});
        let out = render(&doc);
        assert!(
            out.contains("[SELECTOR_VERSION_MISSING] C@missing: RESOLVE_SELECTOR"),
            "{out}"
        );
        assert!(
            out.contains("[IGNORE_TARGET_NOT_FOUND] P@9: CORRECT_IGNORE_REQUEST"),
            "{out}"
        );
        assert!(!out.contains(" -> ") && !out.contains('{'), "{out}");
    }

    #[test]
    fn row_references_match_whole_indexes_and_exclusions_keep_their_pair() {
        let row = |p: &str| json!({"consumer":"C","consumerVersion":"1","provider":p,"providerVersion":"2"});
        let mut rows = vec![row("wrong"); 11];
        rows[1] = row("one");
        rows[10] = row("ten");
        let doc = json!({"decision":"allowed","rows":rows,"resolution":{"excluded":[{"evidence":row("excluded")}]},"reasons":[
            {"code":"ONE","evidenceRefs":["/rows/1/contract","/rows/1/verification"],"actionIds":[]},
            {"code":"TEN","evidenceRefs":["/rows/10"],"actionIds":[]},
            {"code":"EXCLUDED","evidenceRefs":["/resolution/excluded/0"],"actionIds":[]}
        ],"nextActions":[]});
        let out = render(&doc);
        assert!(out.contains("[ONE] C@1 -> one@2: no action"), "{out}");
        assert!(out.contains("[TEN] C@1 -> ten@2: no action"), "{out}");
        assert!(
            out.contains("[EXCLUDED] C@1 -> excluded@2: no action"),
            "{out}"
        );
        assert!(!out.contains("wrong"), "{out}");
        assert_eq!(out.matches("one@2").count(), 1);
    }

    #[test]
    fn names_and_unknown_action_messages_cannot_add_lines() {
        let mut doc = example("unknown");
        doc["nextActions"][0]["target"]["consumer"] = json!("Café\nshop");
        doc["nextActions"][0]["type"] = json!("FUTURE\nACTION");
        doc["nextActions"][0]["message"] = json!("Keep\nthis action useful");
        doc["reasons"][0]["code"] = json!("NEW\nCODE");
        let out = render(&doc);
        assert_eq!(out.lines().count(), 2, "{out}");
        assert!(
            out.contains("[NEW CODE] Café shop@1 -> Payments@2: FUTURE ACTION"),
            "{out}"
        );
    }
    #[test]
    fn unresolved_target_and_selector_constraints_remain_explicit() {
        let doc = json!({"decision":"unknown","rows":[],"reasons":[
            {"code":"TARGET_VERSION_UNRESOLVED","actionIds":["a"],"evidenceRefs":[]},
            {"code":"SELECTOR_VERSION_MISSING","actionIds":["b"],"evidenceRefs":[]},
            {"code":"SELECTOR_VERSION_MISSING","actionIds":["c"],"evidenceRefs":[]}
        ],"nextActions":[
            {"id":"a","type":"RESOLVE_TARGET_VERSION","target":{"consumer":"C","consumerVersion":"1","provider":"P","providerVersion":null,"environment":"prod","tag":"dev"},"evidenceRefs":[]},
            {"id":"b","type":"RESOLVE_SELECTOR","target":{"requested":{"pacticipant":"C","version":null,"latest":true,"branch":"main","tag":"dev","mainBranch":false}},"evidenceRefs":[]},
            {"id":"c","type":"RESOLVE_SELECTOR","target":{"requested":{"pacticipant":"D","version":null,"latest":false,"mainBranch":true}},"evidenceRefs":[]}
        ]});
        let out = render(&doc);
        assert!(
            out.contains("C@1 -> P@?: RESOLVE_TARGET_VERSION P@? (environment prod; tag dev)"),
            "{out}"
        );
        assert!(
            out.contains("C: RESOLVE_SELECTOR C (branch main; tag dev; latest)"),
            "{out}"
        );
        assert!(out.contains("D: RESOLVE_SELECTOR D (main branch)"), "{out}");
    }

    #[test]
    fn reference_only_subjects_and_unknown_action_messages_are_useful() {
        let doc = json!({"decision":"blocked","rows":[],"resolution":{"selectors":[{"requested":{"pacticipant":"C","version":"4"}}],"ignores":[{"pacticipant":"P","version":null}]},"reasons":[
            {"code":"SELECTOR","evidenceRefs":["/resolution/selectors/0"],"actionIds":[]},
            {"code":"IGNORE","evidenceRefs":["/resolution/ignores/0"],"actionIds":[]},
            {"code":"FUTURE","evidenceRefs":[],"actionIds":["a"]}
        ],"nextActions":[{"id":"a","type":"NEW_ACTION","message":"Ask\nthe owner","target":{},"evidenceRefs":[]}]});
        let out = render(&doc);
        assert!(out.contains("[SELECTOR] C@4: no action"), "{out}");
        assert!(out.contains("[IGNORE] P: no action"), "{out}");
        assert!(
            out.contains("[FUTURE] no recorded pair: NEW_ACTION Ask the owner"),
            "{out}"
        );
        assert_eq!(out.lines().count(), 4);
    }
}
