//! Queries behind the matrix and can-i-deploy.
use super::{Db, pacts::PactPublication, verifications::Verification, versions::Version};

macro_rules! publication_query {
    ($tail:literal) => {
        concat!(
            "SELECT pp.id, c.id AS consumer_id, c.name AS consumer_name, pp.consumer_version_id,
                    v.number AS consumer_version_number, c.main_branch AS consumer_main_branch, p.id AS provider_id, p.name AS provider_name,
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

macro_rules! verification_query {
    ($tail:literal) => {
        concat!(
            "SELECT vf.id, vf.number, vf.pact_version_id, vf.provider_version_id, p.name AS provider_name,
                    pv.number AS provider_version_number, vf.consumer_version_id, vf.success,
                    vf.test_results, vf.verified_by, vf.build_url, vf.created_at
             FROM verifications vf
             JOIN versions pv ON pv.id = vf.provider_version_id
             JOIN pacticipants p ON p.id = pv.pacticipant_id ",
            $tail
        )
    };
}

impl Db {
    /// The pacts a consumer version published, ordered by provider name.
    pub async fn publications_for_consumer_version(
        &self,
        consumer_version_id: i64,
    ) -> sqlx::Result<Vec<PactPublication>> {
        sqlx::query_as(publication_query!(
            "WHERE pp.consumer_version_id = ? ORDER BY p.name"
        ))
        .bind(consumer_version_id)
        .fetch_all(&self.pool)
        .await
    }

    /// All pacts published by any version of a consumer, ordered by provider then
    /// newest consumer version first.
    pub async fn publications_for_consumer(
        &self,
        consumer_id: i64,
    ) -> sqlx::Result<Vec<PactPublication>> {
        sqlx::query_as(publication_query!(
            "WHERE c.id = ? ORDER BY p.name, v.id DESC"
        ))
        .bind(consumer_id)
        .fetch_all(&self.pool)
        .await
    }

    /// All pacts published for a provider, ordered by consumer then newest version first.
    pub async fn publications_for_provider(
        &self,
        provider_id: i64,
    ) -> sqlx::Result<Vec<PactPublication>> {
        sqlx::query_as(publication_query!(
            "WHERE pp.provider_id = ? ORDER BY c.name, v.id DESC"
        ))
        .bind(provider_id)
        .fetch_all(&self.pool)
        .await
    }

    /// Consumer names with pacts for a provider, ordered.
    pub async fn consumers_of_provider(
        &self,
        provider_id: i64,
    ) -> sqlx::Result<Vec<(i64, String)>> {
        sqlx::query_as(
            "SELECT DISTINCT c.id, c.name FROM pact_publications pp
             JOIN versions v ON v.id = pp.consumer_version_id
             JOIN pacticipants c ON c.id = v.pacticipant_id
             WHERE pp.provider_id = ? ORDER BY c.name",
        )
        .bind(provider_id)
        .fetch_all(&self.pool)
        .await
    }

    /// Provider names with pacts from a consumer, ordered.
    pub async fn providers_of_consumer(
        &self,
        consumer_id: i64,
    ) -> sqlx::Result<Vec<(i64, String)>> {
        sqlx::query_as(
            "SELECT DISTINCT p.id, p.name FROM pact_publications pp
             JOIN versions v ON v.id = pp.consumer_version_id
             JOIN pacticipants p ON p.id = pp.provider_id
             WHERE v.pacticipant_id = ? ORDER BY p.name",
        )
        .bind(consumer_id)
        .fetch_all(&self.pool)
        .await
    }

    /// The most recent result of a pact content by one provider version.
    pub async fn latest_verification_by_provider_version(
        &self,
        pact_version_id: i64,
        provider_version_id: i64,
    ) -> sqlx::Result<Option<Verification>> {
        sqlx::query_as(verification_query!(
            "WHERE vf.pact_version_id = ? AND vf.provider_version_id = ? ORDER BY vf.id DESC LIMIT 1"
        ))
        .bind(pact_version_id)
        .bind(provider_version_id)
        .fetch_optional(&self.pool)
        .await
    }

    /// The latest result of a pact content per provider version, newest provider
    /// version first.
    pub async fn latest_verifications_per_provider_version(
        &self,
        pact_version_id: i64,
    ) -> sqlx::Result<Vec<Verification>> {
        sqlx::query_as(verification_query!(
            "WHERE vf.pact_version_id = ?1 AND vf.id = (SELECT MAX(id) FROM verifications WHERE pact_version_id = ?1 AND provider_version_id = vf.provider_version_id)
             ORDER BY pv.id DESC"
        ))
        .bind(pact_version_id)
        .fetch_all(&self.pool)
        .await
    }

    /// Versions of a pacticipant, newest first.
    pub async fn versions_of(&self, pacticipant_id: i64) -> sqlx::Result<Vec<Version>> {
        self.list_versions(pacticipant_id).await
    }

    /// Whether the version is the newest one carrying the tag.
    pub async fn is_latest_with_tag(&self, version: &Version, tag: &str) -> sqlx::Result<bool> {
        Ok(self
            .latest_version_with_tag(version.pacticipant_id, tag)
            .await?
            .is_some_and(|v| v.id == version.id))
    }
}
