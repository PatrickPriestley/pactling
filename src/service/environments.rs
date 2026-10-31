//! Environments, deployments and releases. Validation strings and rules follow the Phase 4
//! capture in the implementation plan.
use serde_json::{Map, Value, json};

use crate::db::{
    Db,
    environments::{DeployedVersion, Environment, ReleasedVersion},
};
use crate::error::{Error, Result};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnvironmentRequest {
    pub name: String,
    pub display_name: Option<String>,
    pub production: bool,
    pub contacts: Option<Value>,
}

impl EnvironmentRequest {
    /// Validates a create or update body with the Ruby broker's messages (verified:
    /// `is missing`, `cannot contain spaces`, `must be one of: true, false`).
    pub fn parse(body: &Value) -> Result<EnvironmentRequest> {
        let mut errors: Map<String, Value> = Map::new();
        let name = match body.get("name") {
            Some(Value::String(s)) if s.contains(' ') => {
                errors.insert("name".into(), json!(["cannot contain spaces"]));
                None
            }
            Some(Value::String(s)) if !s.is_empty() => Some(s.clone()),
            _ => {
                errors.insert("name".into(), json!(["is missing"]));
                None
            }
        };
        let production = match body.get("production") {
            Some(Value::Bool(b)) => Some(*b),
            None | Some(Value::Null) => {
                errors.insert("production".into(), json!(["is missing"]));
                None
            }
            Some(_) => {
                errors.insert("production".into(), json!(["must be one of: true, false"]));
                None
            }
        };
        if !errors.is_empty() {
            return Err(Error::Validation(Value::Object(errors)));
        }
        Ok(EnvironmentRequest {
            name: name.expect("validated"),
            display_name: body
                .get("displayName")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(str::to_owned),
            production: production.expect("validated"),
            contacts: body.get("contacts").filter(|c| c.is_array()).cloned(),
        })
    }
}

fn duplicate_name(name: &str) -> Error {
    Error::Validation(
        json!({ "name": [format!("name '{name}' is already used by an existing environment.")] }),
    )
}

pub async fn create(db: &Db, req: &EnvironmentRequest) -> Result<Environment> {
    if db.find_environment_by_name(&req.name).await?.is_some() {
        return Err(duplicate_name(&req.name));
    }
    let contacts = req
        .contacts
        .as_ref()
        .map(|c| serde_json::to_string(c).expect("json"));
    Ok(db
        .create_environment(
            &req.name,
            req.display_name.as_deref(),
            req.production,
            contacts.as_deref(),
        )
        .await?)
}

pub async fn update(db: &Db, uuid: &str, req: &EnvironmentRequest) -> Result<Environment> {
    let existing = db
        .find_environment_by_uuid(uuid)
        .await?
        .ok_or(Error::NoRoute)?;
    if let Some(other) = db.find_environment_by_name(&req.name).await?
        && other.id != existing.id
    {
        return Err(duplicate_name(&req.name));
    }
    let contacts = req
        .contacts
        .as_ref()
        .map(|c| serde_json::to_string(c).expect("json"));
    Ok(db
        .update_environment(
            existing.id,
            &req.name,
            req.display_name.as_deref(),
            req.production,
            contacts.as_deref(),
        )
        .await?)
}

pub async fn find(db: &Db, uuid: &str) -> Result<Environment> {
    db.find_environment_by_uuid(uuid)
        .await?
        .ok_or(Error::NoRoute)
}

pub async fn delete(db: &Db, uuid: &str) -> Result<()> {
    let e = find(db, uuid).await?;
    db.delete_environment(e.id).await?;
    Ok(())
}

/// `POST /pacticipants/{p}/versions/{v}/deployed-versions/environment/{uuid}`.
pub async fn record_deployment(
    db: &Db,
    pacticipant: &str,
    version: &str,
    environment_uuid: &str,
    application_instance: Option<&str>,
) -> Result<(DeployedVersion, Environment, bool)> {
    let v = db
        .find_version(pacticipant, version)
        .await?
        .ok_or(Error::NoRoute)?;
    let e = find(db, environment_uuid).await?;
    let (dv, created) = db
        .record_deployment(v.id, v.pacticipant_id, e.id, application_instance)
        .await?;
    Ok((dv, e, created))
}

pub async fn record_release(
    db: &Db,
    pacticipant: &str,
    version: &str,
    environment_uuid: &str,
) -> Result<(ReleasedVersion, Environment, bool)> {
    let v = db
        .find_version(pacticipant, version)
        .await?
        .ok_or(Error::NoRoute)?;
    let e = find(db, environment_uuid).await?;
    let (rv, created) = db.record_release(v.id, e.id).await?;
    Ok((rv, e, created))
}

/// `PATCH /deployed-versions/{uuid}` with `{"currentlyDeployed": false}`.
pub async fn patch_deployed_version(db: &Db, uuid: &str, body: &Value) -> Result<DeployedVersion> {
    let dv = db
        .find_deployed_version(uuid)
        .await?
        .ok_or(Error::NoRoute)?;
    match body.get("currentlyDeployed") {
        Some(Value::Bool(false)) => Ok(db.mark_undeployed(dv.id).await?),
        Some(Value::Bool(true)) | None | Some(Value::Null) => Ok(dv),
        Some(_) => Err(Error::Validation(
            json!({ "currentlyDeployed": ["must be one of: true, false"] }),
        )),
    }
}

/// `PATCH /released-versions/{uuid}` with `{"currentlySupported": false}`.
pub async fn patch_released_version(db: &Db, uuid: &str, body: &Value) -> Result<ReleasedVersion> {
    let rv = db
        .find_released_version(uuid)
        .await?
        .ok_or(Error::NoRoute)?;
    match body.get("currentlySupported") {
        Some(Value::Bool(false)) => Ok(db.mark_support_ended(rv.id).await?),
        Some(Value::Bool(true)) | None | Some(Value::Null) => Ok(rv),
        Some(_) => Err(Error::Validation(
            json!({ "currentlySupported": ["must be one of: true, false"] }),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn environment_validation_matches_ruby() {
        let err = EnvironmentRequest::parse(&json!({})).unwrap_err();
        assert!(
            matches!(err, Error::Validation(v) if v == json!({"name": ["is missing"], "production": ["is missing"]}))
        );
        let err = EnvironmentRequest::parse(&json!({"name": "bad name!", "production": "yes"}))
            .unwrap_err();
        assert!(
            matches!(err, Error::Validation(v) if v == json!({"name": ["cannot contain spaces"], "production": ["must be one of: true, false"]}))
        );
        let err = EnvironmentRequest::parse(&json!({"name": "ok", "displayName": ""})).unwrap_err();
        assert!(matches!(err, Error::Validation(v) if v == json!({"production": ["is missing"]})));
        let ok = EnvironmentRequest::parse(&json!({"name": "test", "displayName": "Test", "production": false, "contacts": [{"name": "QA"}]})).unwrap();
        assert_eq!(ok.display_name.as_deref(), Some("Test"));
        assert_eq!(ok.contacts, Some(json!([{"name": "QA"}])));
    }

    async fn db() -> Db {
        let db = Db::connect("sqlite::memory:").await.unwrap();
        db.migrate().await.unwrap();
        db
    }

    #[tokio::test]
    async fn recording_a_newer_deployment_undeploys_the_earlier_one() {
        let db = db().await;
        let env = create(
            &db,
            &EnvironmentRequest {
                name: "test".into(),
                display_name: None,
                production: false,
                contacts: None,
            },
        )
        .await
        .unwrap();
        crate::service::versions::put(&db, "C", "1", None)
            .await
            .unwrap();
        crate::service::versions::put(&db, "C", "2", None)
            .await
            .unwrap();
        let (first, _, created) = record_deployment(&db, "C", "1", &env.uuid, None)
            .await
            .unwrap();
        assert!(created && first.currently_deployed);
        let (again, _, created) = record_deployment(&db, "C", "1", &env.uuid, None)
            .await
            .unwrap();
        assert!(!created, "same version again is not a new record");
        assert_eq!(again.uuid, first.uuid);
        let (second, _, created) = record_deployment(&db, "C", "2", &env.uuid, None)
            .await
            .unwrap();
        assert!(created);
        let first_now = db
            .find_deployed_version(&first.uuid)
            .await
            .unwrap()
            .unwrap();
        assert!(!first_now.currently_deployed && first_now.undeployed_at.is_some());
        assert!(second.currently_deployed);
        let current = db.currently_deployed(env.id, None).await.unwrap();
        assert_eq!(
            current
                .iter()
                .map(|d| d.version_number.as_str())
                .collect::<Vec<_>>(),
            ["2"]
        );
    }

    #[tokio::test]
    async fn application_instances_are_deployed_independently() {
        let db = db().await;
        let env = create(
            &db,
            &EnvironmentRequest {
                name: "test".into(),
                display_name: None,
                production: false,
                contacts: None,
            },
        )
        .await
        .unwrap();
        crate::service::versions::put(&db, "C", "1", None)
            .await
            .unwrap();
        crate::service::versions::put(&db, "C", "2", None)
            .await
            .unwrap();
        record_deployment(&db, "C", "1", &env.uuid, Some("blue"))
            .await
            .unwrap();
        record_deployment(&db, "C", "2", &env.uuid, Some("green"))
            .await
            .unwrap();
        let current = db.currently_deployed(env.id, Some("C")).await.unwrap();
        assert_eq!(current.len(), 2, "different instances coexist");
        record_deployment(&db, "C", "2", &env.uuid, Some("blue"))
            .await
            .unwrap();
        let current = db.currently_deployed(env.id, Some("C")).await.unwrap();
        assert_eq!(
            current
                .iter()
                .map(|d| (d.version_number.as_str(), d.application_instance.as_deref()))
                .collect::<Vec<_>>(),
            [("2", Some("blue")), ("2", Some("green"))]
        );
    }

    #[tokio::test]
    async fn releases_are_idempotent_and_support_can_end() {
        let db = db().await;
        let env = create(
            &db,
            &EnvironmentRequest {
                name: "prod".into(),
                display_name: None,
                production: true,
                contacts: None,
            },
        )
        .await
        .unwrap();
        crate::service::versions::put(&db, "C", "1", None)
            .await
            .unwrap();
        let (rv, _, created) = record_release(&db, "C", "1", &env.uuid).await.unwrap();
        assert!(created);
        let (_, _, created) = record_release(&db, "C", "1", &env.uuid).await.unwrap();
        assert!(!created);
        let ended = patch_released_version(&db, &rv.uuid, &json!({"currentlySupported": false}))
            .await
            .unwrap();
        assert!(!ended.currently_supported && ended.support_ended_at.is_some());
        assert!(
            db.currently_supported(env.id, Some("C"), Some("1"))
                .await
                .unwrap()
                .is_empty()
        );
        let dup = create(
            &db,
            &EnvironmentRequest {
                name: "prod".into(),
                display_name: None,
                production: true,
                contacts: None,
            },
        )
        .await
        .unwrap_err();
        assert!(
            matches!(dup, Error::Validation(v) if v == json!({"name": ["name 'prod' is already used by an existing environment."]}))
        );
    }
}
