//! Human diagnostics for unsuccessful broker requests.
pub(super) fn describe(status: reqwest::StatusCode, body: &[u8]) -> String {
    let detail = structured(body).unwrap_or_else(|| {
        let raw = String::from_utf8_lossy(body);
        let mut chars = raw.chars();
        let mut text: String = chars.by_ref().take(1024).collect();
        if chars.next().is_some() {
            text.push_str("… [truncated]");
        }
        text
    });
    format!("broker returned HTTP {status}\n{detail}")
}

fn structured(body: &[u8]) -> Option<String> {
    let document: serde_json::Value = serde_json::from_slice(body).ok()?;
    let error = document.get("error")?;
    let code = error.get("code")?.as_str()?;
    if code.is_empty() {
        return None;
    }
    let details = error.get("details")?;
    let lines: Vec<String> = match details {
        serde_json::Value::Array(items) => items
            .iter()
            .map(|v| Some(v.as_str()?.to_owned()))
            .collect::<Option<_>>()?,
        serde_json::Value::Object(fields) => {
            let mut lines = vec![];
            for (field, items) in fields {
                for item in items.as_array()? {
                    let message = item.as_str()?;
                    lines.push(if field.is_empty() {
                        message.into()
                    } else {
                        format!("{field}: {message}")
                    });
                }
            }
            lines
        }
        _ => return None,
    };
    Some(
        std::iter::once(code.to_owned())
            .chain(lines.into_iter().map(|line| format!("- {line}")))
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::StatusCode;
    #[test]
    fn structured_errors_report_code_and_each_detail() {
        let text=describe(StatusCode::BAD_REQUEST,br#"{"schemaVersion":"1.0","error":{"code":"INVALID_QUERY","details":["Environment with name 'nope' does not exist","second detail"]}}"#);
        assert_eq!(
            text,
            "broker returned HTTP 400 Bad Request\nINVALID_QUERY\n- Environment with name 'nope' does not exist\n- second detail"
        );
        let text=describe(StatusCode::BAD_REQUEST,br#"{"error":{"code":"INVALID_QUERY","details":{"pacticipant":["is missing","must exist"],"":["target is missing"]}}}"#);
        assert!(
            text.contains(
                "- pacticipant: is missing\n- pacticipant: must exist\n- target is missing"
            ),
            "{text}"
        );
    }
    #[test]
    fn malformed_or_other_errors_fall_back_to_bounded_raw_body() {
        for raw in [
            r#"{}"#,
            r#"{"error":{"code":"","details":[]}}"#,
            r#"{"error":{"code":7,"details":[]}}"#,
            r#"{"error":{"code":"X"}}"#,
            r#"{"error":{"code":"X","details":null}}"#,
            r#"{"error":{"code":"X","details":{"field":["valid",7]}}}"#,
            "<html>bad gateway</html>",
            "not JSON",
            r#"{"error":{"code":"INVALID_QUERY","details":["valid",7]}}"#,
            r#"{"error":{"code":"X","details":{"field":"bad"}}}"#,
            r#"{"error":{"details":[]}}"#,
        ] {
            let text = describe(StatusCode::BAD_GATEWAY, raw.as_bytes());
            assert!(text.ends_with(raw), "{text}");
        }
        let long = "界".repeat(1025);
        let text = describe(StatusCode::BAD_GATEWAY, long.as_bytes());
        assert!(
            text.ends_with(&format!("{}… [truncated]", "界".repeat(1024))),
            "{text}"
        );
        let exact = "x".repeat(1024);
        assert!(describe(StatusCode::BAD_GATEWAY, exact.as_bytes()).ends_with(&exact));
        assert!(!describe(StatusCode::BAD_GATEWAY, exact.as_bytes()).contains("truncated"));
        assert!(describe(StatusCode::BAD_GATEWAY, b"bad\xffbody").ends_with("bad�body"));
    }
}
