//! Verification results and the queries behind pacts-for-verification.
use super::{Db, now, pacts::PactPublication};

#[derive(Clone, Debug, PartialEq, Eq, sqlx::FromRow)]
pub struct Verification {
    pub id: i64,
    pub number: i64,
    pub pact_version_id: i64,
    pub provider_version_id: i64,
    pub provider_name: String,
    pub provider_version_number: String,
    pub consumer_version_id: Option<i64>,
    pub success: bool,
    pub test_results: Option<String>,
    pub verified_by: Option<String>,
    pub build_url: Option<String>,
    pub created_at: String,
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

impl Db {
    /// Records a result and returns it with its broker-wide number.
    #[allow(clippy::too_many_arguments)]
    pub async fn create_verification(
        &self,
        pact_version_id: i64,
        provider_version_id: i64,
        consumer_version_id: Option<i64>,
        success: bool,
        test_results: Option<&str>,
        verified_by: Option<&str>,
        build_url: Option<&str>,
    ) -> sqlx::Result<Verification> {
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO verifications (number, pact_version_id, provider_version_id, consumer_version_id, success,
                                        test_results, verified_by, build_url, created_at)
             VALUES ((SELECT COALESCE(MAX(number), 0) + 1 FROM verifications), ?, ?, ?, ?, ?, ?, ?, ?)
             RETURNING id",
        )
        .bind(pact_version_id)
        .bind(provider_version_id)
        .bind(consumer_version_id)
        .bind(success)
        .bind(test_results)
        .bind(verified_by)
        .bind(build_url)
        .bind(now())
        .fetch_one(&self.pool)
        .await?;
        sqlx::query_as(verification_query!("WHERE vf.id = ?"))
            .bind(id)
            .fetch_one(&self.pool)
            .await
    }

    /// Whether any pact between the consumer and provider has ever been verified
    /// (verified: the Ruby broker's `any_verifications?` drives the publish next steps).
    pub async fn any_verifications_for_integration(
        &self,
        consumer_id: i64,
        provider_id: i64,
    ) -> sqlx::Result<bool> {
        let n: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM verifications vf
             JOIN pact_publications pp ON pp.pact_version_id = vf.pact_version_id
             JOIN versions v ON v.id = pp.consumer_version_id
             WHERE v.pacticipant_id = ? AND pp.provider_id = ?",
        )
        .bind(consumer_id)
        .bind(provider_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(n > 0)
    }

    /// The latest verification of a pact content by a provider version on the given branch
    /// (verified: `latest_main_branch_verification` on the Ruby broker's publications).
    pub async fn latest_verification_from_branch(
        &self,
        pact_version_id: i64,
        provider_id: i64,
        branch: &str,
    ) -> sqlx::Result<Option<Verification>> {
        sqlx::query_as(verification_query!(
            "JOIN branch_versions bv ON bv.version_id = pv.id
             JOIN branches b ON b.id = bv.branch_id
             WHERE vf.pact_version_id = ? AND pv.pacticipant_id = ? AND b.name = ?
             ORDER BY vf.id DESC LIMIT 1"
        ))
        .bind(pact_version_id)
        .bind(provider_id)
        .bind(branch)
        .fetch_optional(&self.pool)
        .await
    }

    pub async fn find_verification_by_id(&self, id: i64) -> sqlx::Result<Option<Verification>> {
        sqlx::query_as(verification_query!("WHERE vf.id = ?"))
            .bind(id)
            .fetch_optional(&self.pool)
            .await
    }

    pub async fn find_verification_by_number(
        &self,
        number: i64,
    ) -> sqlx::Result<Option<Verification>> {
        sqlx::query_as(verification_query!("WHERE vf.number = ?"))
            .bind(number)
            .fetch_optional(&self.pool)
            .await
    }

    /// The most recent result for a pact content.
    pub async fn latest_verification_for_pact_version(
        &self,
        pact_version_id: i64,
    ) -> sqlx::Result<Option<Verification>> {
        sqlx::query_as(verification_query!(
            "WHERE vf.pact_version_id = ? ORDER BY vf.id DESC LIMIT 1"
        ))
        .bind(pact_version_id)
        .fetch_optional(&self.pool)
        .await
    }

    /// Whether a provider version on `branch` has successfully verified the content.
    pub async fn verified_by_provider_branch(
        &self,
        pact_version_id: i64,
        provider_id: i64,
        branch: &str,
    ) -> sqlx::Result<bool> {
        sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM verifications vf
                JOIN versions pv ON pv.id = vf.provider_version_id
                JOIN branch_versions bv ON bv.version_id = pv.id
                JOIN branches b ON b.id = bv.branch_id
                WHERE vf.pact_version_id = ? AND pv.pacticipant_id = ? AND b.name = ? AND vf.success = 1)",
        )
        .bind(pact_version_id)
        .bind(provider_id)
        .bind(branch)
        .fetch_one(&self.pool)
        .await
    }

    /// Whether any version of the provider has successfully verified the content.
    pub async fn verified_by_any_provider_version(
        &self,
        pact_version_id: i64,
        provider_id: i64,
    ) -> sqlx::Result<bool> {
        sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM verifications vf
                JOIN versions pv ON pv.id = vf.provider_version_id
                WHERE vf.pact_version_id = ? AND pv.pacticipant_id = ? AND vf.success = 1)",
        )
        .bind(pact_version_id)
        .bind(provider_id)
        .fetch_one(&self.pool)
        .await
    }

    /// Whether a provider version tagged `tag` has successfully verified the content.
    pub async fn verified_by_provider_tag(
        &self,
        pact_version_id: i64,
        provider_id: i64,
        tag: &str,
    ) -> sqlx::Result<bool> {
        sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM verifications vf
                JOIN versions pv ON pv.id = vf.provider_version_id
                JOIN tags t ON t.version_id = pv.id
                WHERE vf.pact_version_id = ? AND pv.pacticipant_id = ? AND t.name = ? AND vf.success = 1)",
        )
        .bind(pact_version_id)
        .bind(provider_id)
        .bind(tag)
        .fetch_one(&self.pool)
        .await
    }

    /// Whether the provider has any version on the branch.
    pub async fn provider_branch_has_versions(
        &self,
        provider_id: i64,
        branch: &str,
    ) -> sqlx::Result<bool> {
        sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM branch_versions bv JOIN branches b ON b.id = bv.branch_id
                            WHERE b.pacticipant_id = ? AND b.name = ?)",
        )
        .bind(provider_id)
        .bind(branch)
        .fetch_one(&self.pool)
        .await
    }

    // --- consumer version selection for a provider -------------------------------------

    /// Each consumer's latest pact for the provider from the consumer's main branch.
    pub async fn provider_pacts_latest_main_branch(
        &self,
        provider_id: i64,
    ) -> sqlx::Result<Vec<PactPublication>> {
        sqlx::query_as(publication_query!(
            "WHERE pp.provider_id = ?1 AND c.main_branch IS NOT NULL AND v.id = (
                SELECT MAX(v2.id) FROM pact_publications pp2 JOIN versions v2 ON v2.id = pp2.consumer_version_id
                JOIN branch_versions bv2 ON bv2.version_id = v2.id JOIN branches b2 ON b2.id = bv2.branch_id AND b2.name = c.main_branch
                WHERE v2.pacticipant_id = c.id AND pp2.provider_id = pp.provider_id)
             ORDER BY c.name"
        ))
        .bind(provider_id)
        .fetch_all(&self.pool)
        .await
    }

    /// Each consumer's latest pact for the provider from the named branch, optionally for
    /// one consumer only.
    pub async fn provider_pacts_latest_branch(
        &self,
        provider_id: i64,
        branch: &str,
        consumer: Option<&str>,
    ) -> sqlx::Result<Vec<PactPublication>> {
        sqlx::query_as(publication_query!(
            "WHERE pp.provider_id = ?1 AND (?3 IS NULL OR c.name = ?3) AND v.id = (
                SELECT MAX(v2.id) FROM pact_publications pp2 JOIN versions v2 ON v2.id = pp2.consumer_version_id
                JOIN branch_versions bv2 ON bv2.version_id = v2.id JOIN branches b2 ON b2.id = bv2.branch_id AND b2.name = ?2
                WHERE v2.pacticipant_id = c.id AND pp2.provider_id = pp.provider_id)
             ORDER BY c.name"
        ))
        .bind(provider_id)
        .bind(branch)
        .bind(consumer)
        .fetch_all(&self.pool)
        .await
    }

    /// Each consumer's latest pact for the provider, any branch.
    pub async fn provider_pacts_latest(
        &self,
        provider_id: i64,
        consumer: Option<&str>,
    ) -> sqlx::Result<Vec<PactPublication>> {
        sqlx::query_as(publication_query!(
            "WHERE pp.provider_id = ?1 AND (?2 IS NULL OR c.name = ?2) AND v.id = (
                SELECT MAX(v2.id) FROM pact_publications pp2 JOIN versions v2 ON v2.id = pp2.consumer_version_id
                WHERE v2.pacticipant_id = c.id AND pp2.provider_id = pp.provider_id)
             ORDER BY c.name"
        ))
        .bind(provider_id)
        .bind(consumer)
        .fetch_all(&self.pool)
        .await
    }

    /// Pacts for the provider from consumer versions carrying the tag: all of them, or
    /// each consumer's latest.
    pub async fn provider_pacts_with_tag(
        &self,
        provider_id: i64,
        tag: &str,
        latest_only: bool,
        consumer: Option<&str>,
    ) -> sqlx::Result<Vec<PactPublication>> {
        let sql = if latest_only {
            publication_query!(
                "WHERE pp.provider_id = ?1 AND (?3 IS NULL OR c.name = ?3) AND v.id = (
                    SELECT MAX(v2.id) FROM pact_publications pp2 JOIN versions v2 ON v2.id = pp2.consumer_version_id
                    JOIN tags t2 ON t2.version_id = v2.id AND t2.name = ?2
                    WHERE v2.pacticipant_id = c.id AND pp2.provider_id = pp.provider_id)
                 ORDER BY c.name"
            )
        } else {
            publication_query!(
                "WHERE pp.provider_id = ?1 AND (?3 IS NULL OR c.name = ?3)
                   AND EXISTS (SELECT 1 FROM tags t WHERE t.version_id = v.id AND t.name = ?2)
                 ORDER BY c.name, v.id"
            )
        };
        sqlx::query_as(sql)
            .bind(provider_id)
            .bind(tag)
            .bind(consumer)
            .fetch_all(&self.pool)
            .await
    }

    pub async fn find_publication_by_id(&self, id: i64) -> sqlx::Result<Option<PactPublication>> {
        sqlx::query_as(publication_query!("WHERE pp.id = ?"))
            .bind(id)
            .fetch_optional(&self.pool)
            .await
    }

    /// The pact publications (one per consumer version) sharing a pact content.
    pub async fn publications_for_pact_version(
        &self,
        pact_version_id: i64,
    ) -> sqlx::Result<Vec<PactPublication>> {
        sqlx::query_as(publication_query!(
            "WHERE pp.pact_version_id = ? ORDER BY v.id DESC"
        ))
        .bind(pact_version_id)
        .fetch_all(&self.pool)
        .await
    }

    /// The latest verification of each provider's pact for a consumer version.
    pub async fn latest_verifications_for_consumer_version(
        &self,
        consumer_version_id: i64,
    ) -> sqlx::Result<Vec<(String, Option<Verification>)>> {
        let publications: Vec<PactPublication> = sqlx::query_as(publication_query!(
            "WHERE pp.consumer_version_id = ? ORDER BY p.name"
        ))
        .bind(consumer_version_id)
        .fetch_all(&self.pool)
        .await?;
        let mut out = Vec::with_capacity(publications.len());
        for p in publications {
            let v = self
                .latest_verification_for_pact_version(p.pact_version_id)
                .await?;
            out.push((p.provider_name, v));
        }
        Ok(out)
    }
}
