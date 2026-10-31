//! List queries over pact publications: per provider, per integration, and the latest of
//! every integration. Ordering follows the captured documents (consumer name, then
//! consumer version order; integration lists newest first except branch lists).
use super::{Db, pacts::PactPublication};

/// Which consumer versions a provider list covers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Selection {
    All,
    Tag(String),
    /// `None` means each consumer's main branch.
    Branch(Option<String>),
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
    /// The latest pact of every integration, ordered by consumer then provider name.
    pub async fn latest_pacts(&self) -> sqlx::Result<Vec<PactPublication>> {
        sqlx::query_as(publication_query!(
            "WHERE v.id = (SELECT MAX(v2.id) FROM pact_publications pp2 JOIN versions v2 ON v2.id = pp2.consumer_version_id
                           WHERE v2.pacticipant_id = c.id AND pp2.provider_id = p.id)
             ORDER BY c.name, p.name"
        ))
        .fetch_all(&self.pool)
        .await
    }

    /// Integrations in the order they were first published.
    pub async fn integrations(&self) -> sqlx::Result<Vec<(String, String)>> {
        sqlx::query_as(
            "SELECT c.name, p.name FROM pact_publications pp
             JOIN versions v ON v.id = pp.consumer_version_id
             JOIN pacticipants c ON c.id = v.pacticipant_id
             JOIN pacticipants p ON p.id = pp.provider_id
             GROUP BY c.id, p.id ORDER BY MIN(pp.id)",
        )
        .fetch_all(&self.pool)
        .await
    }

    /// A provider's pacts from the selected consumer versions; `latest_per_consumer`
    /// keeps only each consumer's newest. Ordered by consumer name, then version order.
    pub async fn pacts_for_provider(
        &self,
        provider: &str,
        selection: &Selection,
        latest_per_consumer: bool,
    ) -> sqlx::Result<Vec<PactPublication>> {
        let (sql, param): (&'static str, Option<&str>) = match (selection, latest_per_consumer) {
            (Selection::All, false) => (publication_query!("WHERE p.name = ?1 ORDER BY c.name, v.id"), None),
            (Selection::All, true) => (
                publication_query!(
                    "WHERE p.name = ?1 AND v.id = (SELECT MAX(v2.id) FROM pact_publications pp2 JOIN versions v2 ON v2.id = pp2.consumer_version_id
                                                    WHERE v2.pacticipant_id = c.id AND pp2.provider_id = p.id)
                     ORDER BY c.name"
                ),
                None,
            ),
            (Selection::Tag(t), false) => (
                publication_query!("WHERE p.name = ?1 AND EXISTS (SELECT 1 FROM tags t WHERE t.version_id = v.id AND t.name = ?2) ORDER BY c.name, v.id"),
                Some(t.as_str()),
            ),
            (Selection::Tag(t), true) => (
                publication_query!(
                    "WHERE p.name = ?1 AND v.id = (SELECT MAX(v2.id) FROM pact_publications pp2 JOIN versions v2 ON v2.id = pp2.consumer_version_id
                                                    JOIN tags t2 ON t2.version_id = v2.id AND t2.name = ?2
                                                    WHERE v2.pacticipant_id = c.id AND pp2.provider_id = p.id)
                     ORDER BY c.name"
                ),
                Some(t.as_str()),
            ),
            (Selection::Branch(Some(b)), false) => (
                publication_query!(
                    "WHERE p.name = ?1 AND EXISTS (SELECT 1 FROM branch_versions bv JOIN branches b ON b.id = bv.branch_id
                                                   WHERE bv.version_id = v.id AND b.name = ?2)
                     ORDER BY c.name, v.id"
                ),
                Some(b.as_str()),
            ),
            (Selection::Branch(Some(b)), true) => (
                publication_query!(
                    "WHERE p.name = ?1 AND v.id = (SELECT MAX(v2.id) FROM pact_publications pp2 JOIN versions v2 ON v2.id = pp2.consumer_version_id
                                                    JOIN branch_versions bv2 ON bv2.version_id = v2.id JOIN branches b2 ON b2.id = bv2.branch_id AND b2.name = ?2
                                                    WHERE v2.pacticipant_id = c.id AND pp2.provider_id = p.id)
                     ORDER BY c.name"
                ),
                Some(b.as_str()),
            ),
            (Selection::Branch(None), false) => (
                publication_query!(
                    "WHERE p.name = ?1 AND EXISTS (SELECT 1 FROM branch_versions bv JOIN branches b ON b.id = bv.branch_id
                                                   WHERE bv.version_id = v.id AND b.name = c.main_branch)
                     ORDER BY c.name, v.id"
                ),
                None,
            ),
            (Selection::Branch(None), true) => (
                publication_query!(
                    "WHERE p.name = ?1 AND v.id = (SELECT MAX(v2.id) FROM pact_publications pp2 JOIN versions v2 ON v2.id = pp2.consumer_version_id
                                                    JOIN branch_versions bv2 ON bv2.version_id = v2.id JOIN branches b2 ON b2.id = bv2.branch_id AND b2.name = c.main_branch
                                                    WHERE v2.pacticipant_id = c.id AND pp2.provider_id = p.id)
                     ORDER BY c.name"
                ),
                None,
            ),
        };
        let mut q = sqlx::query_as(sql).bind(provider);
        if let Some(p) = param {
            q = q.bind(p);
        }
        q.fetch_all(&self.pool).await
    }

    /// An integration's pacts: all of them newest first, or the selected ones. Branch
    /// selections are oldest first, as the Ruby broker lists them.
    pub async fn pacts_for_integration(
        &self,
        provider: &str,
        consumer: &str,
        selection: &Selection,
        latest_only: bool,
    ) -> sqlx::Result<Vec<PactPublication>> {
        let (sql, param): (&'static str, Option<&str>) = match (selection, latest_only) {
            (Selection::All, false) => (publication_query!("WHERE p.name = ?1 AND c.name = ?2 ORDER BY v.id DESC"), None),
            (Selection::All, true) => (publication_query!("WHERE p.name = ?1 AND c.name = ?2 ORDER BY v.id DESC LIMIT 1"), None),
            (Selection::Tag(t), false) => (
                publication_query!("WHERE p.name = ?1 AND c.name = ?2 AND EXISTS (SELECT 1 FROM tags t WHERE t.version_id = v.id AND t.name = ?3) ORDER BY v.id DESC"),
                Some(t.as_str()),
            ),
            (Selection::Tag(t), true) => (
                publication_query!("WHERE p.name = ?1 AND c.name = ?2 AND EXISTS (SELECT 1 FROM tags t WHERE t.version_id = v.id AND t.name = ?3) ORDER BY v.id DESC LIMIT 1"),
                Some(t.as_str()),
            ),
            (Selection::Branch(Some(b)), false) => (
                publication_query!(
                    "WHERE p.name = ?1 AND c.name = ?2 AND EXISTS (SELECT 1 FROM branch_versions bv JOIN branches b ON b.id = bv.branch_id
                                                                   WHERE bv.version_id = v.id AND b.name = ?3)
                     ORDER BY v.id"
                ),
                Some(b.as_str()),
            ),
            (Selection::Branch(Some(b)), true) => (
                publication_query!(
                    "WHERE p.name = ?1 AND c.name = ?2 AND EXISTS (SELECT 1 FROM branch_versions bv JOIN branches b ON b.id = bv.branch_id
                                                                   WHERE bv.version_id = v.id AND b.name = ?3)
                     ORDER BY v.id DESC LIMIT 1"
                ),
                Some(b.as_str()),
            ),
            (Selection::Branch(None), false) => (
                publication_query!(
                    "WHERE p.name = ?1 AND c.name = ?2 AND EXISTS (SELECT 1 FROM branch_versions bv JOIN branches b ON b.id = bv.branch_id
                                                                   WHERE bv.version_id = v.id AND b.name = c.main_branch)
                     ORDER BY v.id"
                ),
                None,
            ),
            (Selection::Branch(None), true) => (
                publication_query!(
                    "WHERE p.name = ?1 AND c.name = ?2 AND EXISTS (SELECT 1 FROM branch_versions bv JOIN branches b ON b.id = bv.branch_id
                                                                   WHERE bv.version_id = v.id AND b.name = c.main_branch)
                     ORDER BY v.id DESC LIMIT 1"
                ),
                None,
            ),
        };
        let mut q = sqlx::query_as(sql).bind(provider).bind(consumer);
        if let Some(p) = param {
            q = q.bind(p);
        }
        q.fetch_all(&self.pool).await
    }

    /// Deletes every publication between the two pacticipants. Returns how many.
    pub async fn delete_integration_pacts(
        &self,
        provider: &str,
        consumer: &str,
    ) -> sqlx::Result<u64> {
        let r = sqlx::query(
            "DELETE FROM pact_publications WHERE id IN (
                SELECT pp.id FROM pact_publications pp
                JOIN versions v ON v.id = pp.consumer_version_id
                JOIN pacticipants c ON c.id = v.pacticipant_id
                JOIN pacticipants p ON p.id = pp.provider_id
                WHERE p.name = ? AND c.name = ?)",
        )
        .bind(provider)
        .bind(consumer)
        .execute(&self.pool)
        .await?;
        Ok(r.rows_affected())
    }
}
