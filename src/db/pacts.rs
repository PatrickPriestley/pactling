//! Pact versions (distinct content per integration) and pact publications (which consumer
//! version published which content for which provider).
use super::{Db, now};

/// A publication joined with everything a pact document needs.
#[derive(Clone, Debug, PartialEq, Eq, sqlx::FromRow)]
pub struct PactPublication {
    pub id: i64,
    pub consumer_id: i64,
    pub consumer_name: String,
    pub consumer_version_id: i64,
    pub consumer_version_number: String,
    pub provider_id: i64,
    pub provider_name: String,
    pub pact_version_id: i64,
    pub sha: String,
    pub content: String,
    pub created_at: String,
    pub updated_at: Option<String>,
}

macro_rules! publication_query {
    ($tail:literal) => {
        concat!(
            "SELECT pp.id, c.id AS consumer_id, c.name AS consumer_name, pp.consumer_version_id,
                    v.number AS consumer_version_number, p.id AS provider_id, p.name AS provider_name,
                    pp.pact_version_id, pv.sha, pv.content, pp.created_at, pp.updated_at
             FROM pact_publications pp
             JOIN versions v ON v.id = pp.consumer_version_id
             JOIN pacticipants c ON c.id = v.pacticipant_id
             JOIN pacticipants p ON p.id = pp.provider_id
             JOIN pact_versions pv ON pv.id = pp.pact_version_id ",
            $tail
        )
    };
}

impl Db {
    pub async fn find_or_create_pact_version(
        &self,
        consumer_id: i64,
        provider_id: i64,
        sha: &str,
        content: &str,
    ) -> sqlx::Result<i64> {
        let inserted: Option<i64> = sqlx::query_scalar(
            "INSERT INTO pact_versions (consumer_id, provider_id, sha, content, created_at) VALUES (?, ?, ?, ?, ?)
             ON CONFLICT(consumer_id, provider_id, sha) DO NOTHING RETURNING id",
        )
        .bind(consumer_id)
        .bind(provider_id)
        .bind(sha)
        .bind(content)
        .bind(now())
        .fetch_optional(&self.pool)
        .await?;
        match inserted {
            Some(id) => Ok(id),
            None => {
                sqlx::query_scalar("SELECT id FROM pact_versions WHERE consumer_id = ? AND provider_id = ? AND sha = ?")
                    .bind(consumer_id)
                    .bind(provider_id)
                    .bind(sha)
                    .fetch_one(&self.pool)
                    .await
            }
        }
    }

    pub async fn find_publication(
        &self,
        consumer_version_id: i64,
        provider_id: i64,
    ) -> sqlx::Result<Option<PactPublication>> {
        sqlx::query_as(publication_query!(
            "WHERE pp.consumer_version_id = ? AND pp.provider_id = ?"
        ))
        .bind(consumer_version_id)
        .bind(provider_id)
        .fetch_optional(&self.pool)
        .await
    }

    /// Inserts the publication unless one already exists for the version and provider
    /// (a concurrent publish may have won). Returns the row that is there afterwards and
    /// whether this call inserted it.
    pub async fn create_publication(
        &self,
        consumer_version_id: i64,
        provider_id: i64,
        pact_version_id: i64,
    ) -> sqlx::Result<(PactPublication, bool)> {
        let inserted: Option<i64> = sqlx::query_scalar(
            "INSERT INTO pact_publications (consumer_version_id, provider_id, pact_version_id, created_at) VALUES (?, ?, ?, ?)
             ON CONFLICT(consumer_version_id, provider_id) DO NOTHING RETURNING id",
        )
        .bind(consumer_version_id)
        .bind(provider_id)
        .bind(pact_version_id)
        .bind(now())
        .fetch_optional(&self.pool)
        .await?;
        let row = self
            .find_publication(consumer_version_id, provider_id)
            .await?
            .expect("inserted or already present");
        Ok((row, inserted.is_some()))
    }

    pub async fn touch_publication(&self, id: i64) -> sqlx::Result<()> {
        sqlx::query("UPDATE pact_publications SET updated_at = ? WHERE id = ?")
            .bind(now())
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn delete_publication(&self, id: i64) -> sqlx::Result<bool> {
        let r = sqlx::query("DELETE FROM pact_publications WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(r.rows_affected() > 0)
    }

    pub async fn find_pact(
        &self,
        provider: &str,
        consumer: &str,
        version: &str,
    ) -> sqlx::Result<Option<PactPublication>> {
        sqlx::query_as(publication_query!(
            "WHERE p.name = ? AND c.name = ? AND v.number = ?"
        ))
        .bind(provider)
        .bind(consumer)
        .bind(version)
        .fetch_optional(&self.pool)
        .await
    }

    pub async fn find_pact_by_sha(
        &self,
        provider: &str,
        consumer: &str,
        sha: &str,
    ) -> sqlx::Result<Option<PactPublication>> {
        sqlx::query_as(publication_query!(
            "WHERE p.name = ? AND c.name = ? AND pv.sha = ? ORDER BY v.id DESC LIMIT 1"
        ))
        .bind(provider)
        .bind(consumer)
        .bind(sha)
        .fetch_optional(&self.pool)
        .await
    }

    /// The pact of the newest consumer version for the integration.
    pub async fn latest_pact(
        &self,
        provider: &str,
        consumer: &str,
    ) -> sqlx::Result<Option<PactPublication>> {
        sqlx::query_as(publication_query!(
            "WHERE p.name = ? AND c.name = ? ORDER BY v.id DESC LIMIT 1"
        ))
        .bind(provider)
        .bind(consumer)
        .fetch_optional(&self.pool)
        .await
    }

    pub async fn latest_pact_with_tag(
        &self,
        provider: &str,
        consumer: &str,
        tag: &str,
    ) -> sqlx::Result<Option<PactPublication>> {
        sqlx::query_as(publication_query!(
            "JOIN tags t ON t.version_id = v.id
             WHERE p.name = ? AND c.name = ? AND t.name = ? ORDER BY v.id DESC LIMIT 1"
        ))
        .bind(provider)
        .bind(consumer)
        .bind(tag)
        .fetch_optional(&self.pool)
        .await
    }

    /// The pact of the newest consumer version that has no tags at all.
    pub async fn latest_pact_untagged(
        &self,
        provider: &str,
        consumer: &str,
    ) -> sqlx::Result<Option<PactPublication>> {
        sqlx::query_as(publication_query!(
            "WHERE p.name = ? AND c.name = ? AND NOT EXISTS (SELECT 1 FROM tags t WHERE t.version_id = v.id)
             ORDER BY v.id DESC LIMIT 1"
        ))
        .bind(provider)
        .bind(consumer)
        .fetch_optional(&self.pool)
        .await
    }

    /// The sha of the newest pact for the integration published by a consumer version
    /// older than `before_version_id` and tagged `tag`.
    pub async fn previous_pact_sha_with_tag(
        &self,
        consumer_id: i64,
        provider_id: i64,
        before_version_id: i64,
        tag: &str,
    ) -> sqlx::Result<Option<String>> {
        sqlx::query_scalar(
            "SELECT pv.sha FROM pact_publications pp
             JOIN versions v ON v.id = pp.consumer_version_id
             JOIN pact_versions pv ON pv.id = pp.pact_version_id
             JOIN tags t ON t.version_id = v.id
             WHERE v.pacticipant_id = ? AND pp.provider_id = ? AND v.id < ? AND t.name = ?
             ORDER BY v.id DESC LIMIT 1",
        )
        .bind(consumer_id)
        .bind(provider_id)
        .bind(before_version_id)
        .bind(tag)
        .fetch_optional(&self.pool)
        .await
    }

    /// As above, for consumer versions that have no tags.
    pub async fn previous_pact_sha_untagged(
        &self,
        consumer_id: i64,
        provider_id: i64,
        before_version_id: i64,
    ) -> sqlx::Result<Option<String>> {
        sqlx::query_scalar(
            "SELECT pv.sha FROM pact_publications pp
             JOIN versions v ON v.id = pp.consumer_version_id
             JOIN pact_versions pv ON pv.id = pp.pact_version_id
             WHERE v.pacticipant_id = ? AND pp.provider_id = ? AND v.id < ?
               AND NOT EXISTS (SELECT 1 FROM tags t WHERE t.version_id = v.id)
             ORDER BY v.id DESC LIMIT 1",
        )
        .bind(consumer_id)
        .bind(provider_id)
        .bind(before_version_id)
        .fetch_optional(&self.pool)
        .await
    }
}
