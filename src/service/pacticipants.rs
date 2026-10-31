//! Pacticipant rules, verified against Ruby broker 2.121.2.
use serde_json::{Value, json};

use crate::db::{Db, pacticipants::Pacticipant};
use crate::error::{Error, Result};

/// Fields a client may set. `Some(None)` clears a field (an empty string in the request);
/// `None` leaves it alone. `name` is only required when creating through `POST`.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Patch {
    pub name: Option<String>,
    pub display_name: Option<Option<String>>,
    pub repository_url: Option<Option<String>>,
    pub main_branch: Option<Option<String>>,
}

impl Patch {
    /// Parses a request body. Unknown fields are ignored; `null` means "not provided";
    /// `""` clears the field.
    pub fn from_json(body: &Value) -> Patch {
        let field = |k: &str| -> Option<Option<String>> {
            match body.get(k) {
                None | Some(Value::Null) => None,
                Some(Value::String(s)) if s.is_empty() => Some(None),
                Some(Value::String(s)) => Some(Some(s.clone())),
                Some(other) => Some(Some(other.to_string())),
            }
        };
        Patch {
            name: field("name").flatten().or_else(|| {
                body.get("name")
                    .and_then(|v| v.as_str())
                    .map(|_| String::new())
            }),
            display_name: field("displayName"),
            repository_url: field("repositoryUrl"),
            main_branch: field("mainBranch"),
        }
    }

    /// The Ruby broker's validation for `POST /pacticipants`.
    pub fn validate_for_create(&self) -> Result<&str> {
        match self.name.as_deref() {
            None => Err(Error::Validation(json!({ "name": ["is missing"] }))),
            Some("") => Err(Error::Validation(json!({ "name": ["must be filled"] }))),
            Some(name) => Ok(name),
        }
    }
}

/// The default display name: split on `_`, `-`, `.` and lower-to-upper camel-case
/// boundaries, then upper-case the first letter of each word and leave the rest alone.
/// Verified: `my-service_v2` becomes `My Service V2`, `ALLCAPS` stays, `camelCaseName`
/// becomes `Camel Case Name`.
pub fn default_display_name(name: &str) -> String {
    let mut spaced = String::with_capacity(name.len() + 4);
    let mut prev_lower = false;
    for c in name.chars() {
        if matches!(c, '_' | '-' | '.') {
            spaced.push(' ');
            prev_lower = false;
            continue;
        }
        if c.is_uppercase() && prev_lower {
            spaced.push(' ');
        }
        prev_lower = c.is_lowercase();
        spaced.push(c);
    }
    spaced
        .split_whitespace()
        .map(|w| {
            let mut chars = w.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect::<String>(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Debug, PartialEq, Eq)]
pub struct Saved {
    pub pacticipant: Pacticipant,
    pub created: bool,
}

/// Create-or-update, as `POST /pacticipants` and `PATCH /pacticipants/{name}` behave.
pub async fn upsert(db: &Db, name: &str, patch: &Patch) -> Result<Saved> {
    match db.find_pacticipant(name).await? {
        Some(_) => Ok(Saved {
            pacticipant: apply_update(db, name, patch).await?,
            created: false,
        }),
        None => {
            let default = default_display_name(name);
            let display_name = match &patch.display_name {
                None => Some(default.as_str()),
                Some(v) => v.as_deref(),
            };
            let pacticipant = db
                .insert_pacticipant(
                    name,
                    display_name,
                    patch.repository_url.as_ref().and_then(|v| v.as_deref()),
                    patch.main_branch.as_ref().and_then(|v| v.as_deref()),
                )
                .await?;
            Ok(Saved {
                pacticipant,
                created: true,
            })
        }
    }
}

/// Update only, as `PUT /pacticipants/{name}` behaves: 404 when missing.
pub async fn update(db: &Db, name: &str, patch: &Patch) -> Result<Pacticipant> {
    if db.find_pacticipant(name).await?.is_none() {
        return Err(not_found(name));
    }
    apply_update(db, name, patch).await
}

pub async fn delete(db: &Db, name: &str) -> Result<()> {
    if db.delete_pacticipant(name).await? {
        Ok(())
    } else {
        Err(not_found(name))
    }
}

pub async fn find(db: &Db, name: &str) -> Result<Pacticipant> {
    db.find_pacticipant(name)
        .await?
        .ok_or_else(|| not_found(name))
}

pub fn not_found(name: &str) -> Error {
    Error::NotFound(format!("No pacticipant with name '{name}' found"))
}

async fn apply_update(db: &Db, name: &str, patch: &Patch) -> Result<Pacticipant> {
    Ok(db
        .update_pacticipant(
            name,
            patch.display_name.as_ref().map(|v| v.as_deref()),
            patch.repository_url.as_ref().map(|v| v.as_deref()),
            patch.main_branch.as_ref().map(|v| v.as_deref()),
        )
        .await?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_name_defaults_match_ruby_broker() {
        for (name, expected) in [
            ("my-service_v2", "My Service V2"),
            ("foo_bar-baz", "Foo Bar Baz"),
            ("ALLCAPS", "ALLCAPS"),
            ("camelCaseName", "Camel Case Name"),
            ("with.dots", "With Dots"),
            ("x", "X"),
            ("Example App", "Example App"),
            ("Bar", "Bar"),
        ] {
            assert_eq!(default_display_name(name), expected, "{name}");
        }
    }

    #[test]
    fn patch_treats_null_as_absent_and_empty_as_clear() {
        let p = Patch::from_json(
            &json!({"displayName": null, "repositoryUrl": "", "mainBranch": "main", "bogus": 1}),
        );
        assert_eq!(p.display_name, None);
        assert_eq!(p.repository_url, Some(None));
        assert_eq!(p.main_branch, Some(Some("main".into())));
    }

    #[test]
    fn create_validation_matches_ruby_broker() {
        let missing = Patch::from_json(&json!({"mainBranch": "main"}));
        assert!(
            matches!(missing.validate_for_create(), Err(Error::Validation(v)) if v == json!({"name": ["is missing"]}))
        );
        let empty = Patch::from_json(&json!({"name": ""}));
        assert!(
            matches!(empty.validate_for_create(), Err(Error::Validation(v)) if v == json!({"name": ["must be filled"]}))
        );
        let ok = Patch::from_json(&json!({"name": "Foo"}));
        assert_eq!(ok.validate_for_create().unwrap(), "Foo");
    }
}
