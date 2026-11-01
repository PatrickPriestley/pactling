//! Environments, deployed versions and released versions.
use super::{Db, now};

#[derive(Clone, Debug, PartialEq, Eq, sqlx::FromRow)]
pub struct Environment {
    pub id: i64,
    pub uuid: String,
    pub name: String,
    pub display_name: Option<String>,
    pub production: bool,
    /// JSON array as sent by the client, kept verbatim.
    pub contacts: Option<String>,
    pub created_at: String,
    pub updated_at: Option<String>,
}

/// A deployment or release record joined with its version, pacticipant and environment.
#[derive(Clone, Debug, PartialEq, Eq, sqlx::FromRow)]
pub struct DeployedVersion {
    pub id: i64,
    pub uuid: String,
    pub version_id: i64,
    pub version_number: String,
    pub pacticipant_id: i64,
    pub pacticipant_name: String,
    pub environment_id: i64,
    pub application_instance: Option<String>,
    pub currently_deployed: bool,
    pub undeployed_at: Option<String>,
    pub created_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq, sqlx::FromRow)]
pub struct ReleasedVersion {
    pub id: i64,
    pub uuid: String,
    pub version_id: i64,
    pub version_number: String,
    pub pacticipant_id: i64,
    pub pacticipant_name: String,
    pub environment_id: i64,
    pub currently_supported: bool,
    pub support_ended_at: Option<String>,
    pub created_at: String,
}

macro_rules! deployed_query {
    ($tail:literal) => {
        concat!(
            "SELECT dv.id, dv.uuid, dv.version_id, v.number AS version_number, p.id AS pacticipant_id,
                    p.name AS pacticipant_name, dv.environment_id, dv.application_instance,
                    dv.currently_deployed, dv.undeployed_at, dv.created_at
             FROM deployed_versions dv
             JOIN versions v ON v.id = dv.version_id
             JOIN pacticipants p ON p.id = v.pacticipant_id ",
            $tail
        )
    };
}

macro_rules! released_query {
    ($tail:literal) => {
        concat!(
            "SELECT rv.id, rv.uuid, rv.version_id, v.number AS version_number, p.id AS pacticipant_id,
                    p.name AS pacticipant_name, rv.environment_id, rv.currently_supported,
                    rv.support_ended_at, rv.created_at
             FROM released_versions rv
             JOIN versions v ON v.id = rv.version_id
             JOIN pacticipants p ON p.id = v.pacticipant_id ",
            $tail
        )
    };
}

impl Db {
    // --- environments -----------------------------------------------------------------

    pub async fn create_environment(
        &self,
        name: &str,
        display_name: Option<&str>,
        production: bool,
        contacts: Option<&str>,
    ) -> sqlx::Result<Environment> {
        let uuid = uuid::Uuid::new_v4().to_string();
        sqlx::query_as(
            "INSERT INTO environments (uuid, name, display_name, production, contacts, created_at) VALUES (?, ?, ?, ?, ?, ?)
             RETURNING id, uuid, name, display_name, production, contacts, created_at, updated_at",
        )
        .bind(uuid)
        .bind(name)
        .bind(display_name)
        .bind(production)
        .bind(contacts)
        .bind(now())
        .fetch_one(&self.conn)
        .await
    }

    pub async fn update_environment(
        &self,
        id: i64,
        name: &str,
        display_name: Option<&str>,
        production: bool,
        contacts: Option<&str>,
    ) -> sqlx::Result<Environment> {
        sqlx::query_as(
            "UPDATE environments SET name = ?, display_name = ?, production = ?, contacts = ?, updated_at = ? WHERE id = ?
             RETURNING id, uuid, name, display_name, production, contacts, created_at, updated_at",
        )
        .bind(name)
        .bind(display_name)
        .bind(production)
        .bind(contacts)
        .bind(now())
        .bind(id)
        .fetch_one(&self.conn)
        .await
    }

    pub async fn find_environment_by_uuid(&self, uuid: &str) -> sqlx::Result<Option<Environment>> {
        sqlx::query_as("SELECT id, uuid, name, display_name, production, contacts, created_at, updated_at FROM environments WHERE uuid = ?")
            .bind(uuid)
            .fetch_optional(&self.conn)
            .await
    }

    pub async fn find_environment_by_name(&self, name: &str) -> sqlx::Result<Option<Environment>> {
        sqlx::query_as("SELECT id, uuid, name, display_name, production, contacts, created_at, updated_at FROM environments WHERE name = ?")
            .bind(name)
            .fetch_optional(&self.conn)
            .await
    }

    pub async fn find_environment_by_id(&self, id: i64) -> sqlx::Result<Option<Environment>> {
        sqlx::query_as("SELECT id, uuid, name, display_name, production, contacts, created_at, updated_at FROM environments WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.conn)
            .await
    }

    /// Ordered by name.
    pub async fn list_environments(&self) -> sqlx::Result<Vec<Environment>> {
        sqlx::query_as("SELECT id, uuid, name, display_name, production, contacts, created_at, updated_at FROM environments ORDER BY name")
            .fetch_all(&self.conn)
            .await
    }

    pub async fn delete_environment(&self, id: i64) -> sqlx::Result<bool> {
        let r = sqlx::query("DELETE FROM environments WHERE id = ?")
            .bind(id)
            .execute(&self.conn)
            .await?;
        Ok(r.rows_affected() > 0)
    }

    // --- deployed versions ------------------------------------------------------------

    /// Records a deployment. If the same version is already currently deployed there (same
    /// application instance) the existing record is returned with `created` false;
    /// otherwise earlier deployments of the pacticipant to that environment and
    /// instance are marked undeployed and a new record is created.
    pub async fn record_deployment(
        &self,
        version_id: i64,
        pacticipant_id: i64,
        environment_id: i64,
        application_instance: Option<&str>,
    ) -> sqlx::Result<(DeployedVersion, bool)> {
        if let Some(existing) = sqlx::query_as::<_, DeployedVersion>(deployed_query!(
            "WHERE dv.version_id = ?1 AND dv.environment_id = ?2 AND dv.currently_deployed = 1
               AND ((?3 IS NULL AND dv.application_instance IS NULL) OR dv.application_instance = ?3)"
        ))
        .bind(version_id)
        .bind(environment_id)
        .bind(application_instance)
        .fetch_optional(&self.conn)
        .await?
        {
            return Ok((existing, false));
        }
        sqlx::query(
            "UPDATE deployed_versions SET currently_deployed = 0, undeployed_at = ?1
             WHERE environment_id = ?2 AND currently_deployed = 1
               AND ((?3 IS NULL AND application_instance IS NULL) OR application_instance = ?3)
               AND version_id IN (SELECT id FROM versions WHERE pacticipant_id = ?4)",
        )
        .bind(now())
        .bind(environment_id)
        .bind(application_instance)
        .bind(pacticipant_id)
        .execute(&self.conn)
        .await?;
        let uuid = uuid::Uuid::new_v4().to_string();
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO deployed_versions (uuid, version_id, environment_id, application_instance, created_at) VALUES (?, ?, ?, ?, ?) RETURNING id",
        )
        .bind(uuid)
        .bind(version_id)
        .bind(environment_id)
        .bind(application_instance)
        .bind(now())
        .fetch_one(&self.conn)
        .await?;
        let row = self
            .find_deployed_version_by_id(id)
            .await?
            .expect("just inserted");
        Ok((row, true))
    }

    async fn find_deployed_version_by_id(&self, id: i64) -> sqlx::Result<Option<DeployedVersion>> {
        sqlx::query_as(deployed_query!("WHERE dv.id = ?"))
            .bind(id)
            .fetch_optional(&self.conn)
            .await
    }

    pub async fn find_deployed_version(&self, uuid: &str) -> sqlx::Result<Option<DeployedVersion>> {
        sqlx::query_as(deployed_query!("WHERE dv.uuid = ?"))
            .bind(uuid)
            .fetch_optional(&self.conn)
            .await
    }

    pub async fn mark_undeployed(&self, id: i64) -> sqlx::Result<DeployedVersion> {
        sqlx::query("UPDATE deployed_versions SET currently_deployed = 0, undeployed_at = ? WHERE id = ? AND currently_deployed = 1")
            .bind(now())
            .bind(id)
            .execute(&self.conn)
            .await?;
        Ok(self.find_deployed_version_by_id(id).await?.expect("exists"))
    }

    /// Currently deployed versions in an environment, newest record first, optionally for
    /// one pacticipant.
    pub async fn currently_deployed(
        &self,
        environment_id: i64,
        pacticipant: Option<&str>,
    ) -> sqlx::Result<Vec<DeployedVersion>> {
        sqlx::query_as(deployed_query!(
            "WHERE dv.environment_id = ?1 AND dv.currently_deployed = 1 AND (?2 IS NULL OR p.name = ?2)
             ORDER BY dv.id DESC"
        ))
        .bind(environment_id)
        .bind(pacticipant)
        .fetch_all(&self.conn)
        .await
    }

    /// Every deployment record of a version, oldest first.
    /// Every version of a pacticipant currently deployed anywhere, oldest record first.
    pub async fn currently_deployed_for_pacticipant(
        &self,
        pacticipant_id: i64,
    ) -> sqlx::Result<Vec<DeployedVersion>> {
        sqlx::query_as(deployed_query!(
            "WHERE p.id = ? AND dv.currently_deployed = 1 ORDER BY dv.id"
        ))
        .bind(pacticipant_id)
        .fetch_all(&self.conn)
        .await
    }

    /// Every version of a pacticipant currently released and supported, oldest first.
    pub async fn currently_supported_for_pacticipant(
        &self,
        pacticipant_id: i64,
    ) -> sqlx::Result<Vec<ReleasedVersion>> {
        sqlx::query_as(released_query!(
            "WHERE p.id = ? AND rv.currently_supported = 1 ORDER BY rv.id"
        ))
        .bind(pacticipant_id)
        .fetch_all(&self.conn)
        .await
    }

    pub async fn deployments_of_version(
        &self,
        version_id: i64,
    ) -> sqlx::Result<Vec<DeployedVersion>> {
        sqlx::query_as(deployed_query!("WHERE dv.version_id = ? ORDER BY dv.id"))
            .bind(version_id)
            .fetch_all(&self.conn)
            .await
    }

    // --- released versions ------------------------------------------------------------

    pub async fn record_release(
        &self,
        version_id: i64,
        environment_id: i64,
    ) -> sqlx::Result<(ReleasedVersion, bool)> {
        if let Some(existing) = sqlx::query_as::<_, ReleasedVersion>(released_query!(
            "WHERE rv.version_id = ? AND rv.environment_id = ?"
        ))
        .bind(version_id)
        .bind(environment_id)
        .fetch_optional(&self.conn)
        .await?
        {
            if !existing.currently_supported {
                sqlx::query("UPDATE released_versions SET currently_supported = 1, support_ended_at = NULL WHERE id = ?")
                    .bind(existing.id)
                    .execute(&self.conn)
                    .await?;
                return Ok((
                    self.find_released_version_by_id(existing.id)
                        .await?
                        .expect("exists"),
                    false,
                ));
            }
            return Ok((existing, false));
        }
        let uuid = uuid::Uuid::new_v4().to_string();
        let id: i64 = sqlx::query_scalar("INSERT INTO released_versions (uuid, version_id, environment_id, created_at) VALUES (?, ?, ?, ?) RETURNING id")
            .bind(uuid)
            .bind(version_id)
            .bind(environment_id)
            .bind(now())
            .fetch_one(&self.conn)
            .await?;
        Ok((
            self.find_released_version_by_id(id)
                .await?
                .expect("just inserted"),
            true,
        ))
    }

    async fn find_released_version_by_id(&self, id: i64) -> sqlx::Result<Option<ReleasedVersion>> {
        sqlx::query_as(released_query!("WHERE rv.id = ?"))
            .bind(id)
            .fetch_optional(&self.conn)
            .await
    }

    pub async fn find_released_version(&self, uuid: &str) -> sqlx::Result<Option<ReleasedVersion>> {
        sqlx::query_as(released_query!("WHERE rv.uuid = ?"))
            .bind(uuid)
            .fetch_optional(&self.conn)
            .await
    }

    pub async fn mark_support_ended(&self, id: i64) -> sqlx::Result<ReleasedVersion> {
        sqlx::query("UPDATE released_versions SET currently_supported = 0, support_ended_at = ? WHERE id = ? AND currently_supported = 1")
            .bind(now())
            .bind(id)
            .execute(&self.conn)
            .await?;
        Ok(self.find_released_version_by_id(id).await?.expect("exists"))
    }

    /// Currently supported released versions in an environment, optionally filtered.
    pub async fn currently_supported(
        &self,
        environment_id: i64,
        pacticipant: Option<&str>,
        version: Option<&str>,
    ) -> sqlx::Result<Vec<ReleasedVersion>> {
        sqlx::query_as(released_query!(
            "WHERE rv.environment_id = ?1 AND rv.currently_supported = 1
               AND (?2 IS NULL OR p.name = ?2) AND (?3 IS NULL OR v.number = ?3)
             ORDER BY rv.id DESC"
        ))
        .bind(environment_id)
        .bind(pacticipant)
        .bind(version)
        .fetch_all(&self.conn)
        .await
    }

    pub async fn releases_of_version(&self, version_id: i64) -> sqlx::Result<Vec<ReleasedVersion>> {
        sqlx::query_as(released_query!("WHERE rv.version_id = ? ORDER BY rv.id"))
            .bind(version_id)
            .fetch_all(&self.conn)
            .await
    }

    /// Environments a version is currently deployed to or released and supported in,
    /// ordered by environment name, without duplicates.
    pub async fn environments_of_version(&self, version_id: i64) -> sqlx::Result<Vec<Environment>> {
        sqlx::query_as(
            "SELECT DISTINCT e.id, e.uuid, e.name, e.display_name, e.production, e.contacts, e.created_at, e.updated_at
             FROM environments e
             WHERE e.id IN (SELECT environment_id FROM deployed_versions WHERE version_id = ?1 AND currently_deployed = 1)
                OR e.id IN (SELECT environment_id FROM released_versions WHERE version_id = ?1 AND currently_supported = 1)
             ORDER BY e.name",
        )
        .bind(version_id)
        .fetch_all(&self.conn)
        .await
    }

    /// Version ids of a pacticipant currently deployed to or released in an environment.
    pub async fn version_ids_in_environment(
        &self,
        pacticipant_id: i64,
        environment_id: i64,
    ) -> sqlx::Result<Vec<i64>> {
        sqlx::query_scalar(
            "SELECT DISTINCT v.id FROM versions v
             WHERE v.pacticipant_id = ?1 AND (
                v.id IN (SELECT version_id FROM deployed_versions WHERE environment_id = ?2 AND currently_deployed = 1)
             OR v.id IN (SELECT version_id FROM released_versions WHERE environment_id = ?2 AND currently_supported = 1))
             ORDER BY v.id DESC",
        )
        .bind(pacticipant_id)
        .bind(environment_id)
        .fetch_all(&self.conn)
        .await
    }

    /// Version ids currently deployed (or released when `include_released`) in an
    /// environment, across all pacticipants.
    pub async fn version_ids_deployed_anywhere(
        &self,
        deployed: bool,
        released: bool,
        environment_id: Option<i64>,
    ) -> sqlx::Result<Vec<i64>> {
        let mut ids: Vec<i64> = vec![];
        if deployed {
            ids.extend(
                sqlx::query_scalar::<_, i64>(
                    "SELECT DISTINCT version_id FROM deployed_versions WHERE currently_deployed = 1 AND (?1 IS NULL OR environment_id = ?1)",
                )
                .bind(environment_id)
                .fetch_all(&self.conn)
                .await?,
            );
        }
        if released {
            ids.extend(
                sqlx::query_scalar::<_, i64>(
                    "SELECT DISTINCT version_id FROM released_versions WHERE currently_supported = 1 AND (?1 IS NULL OR environment_id = ?1)",
                )
                .bind(environment_id)
                .fetch_all(&self.conn)
                .await?,
            );
        }
        ids.sort_unstable();
        ids.dedup();
        Ok(ids)
    }
}
