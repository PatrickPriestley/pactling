//! Versions, branches, branch membership and tags. Versions are ordered by `id`, which
//! is the Ruby broker's `order`: creation order, never semver.
use super::{Db, now};

#[derive(Clone, Debug, PartialEq, Eq, sqlx::FromRow)]
pub struct Version {
    pub id: i64,
    pub pacticipant_id: i64,
    pub pacticipant_name: String,
    pub number: String,
    pub build_url: Option<String>,
    pub created_at: String,
    pub updated_at: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, sqlx::FromRow)]
pub struct Branch {
    pub id: i64,
    pub pacticipant_id: i64,
    pub name: String,
    pub created_at: String,
    pub updated_at: Option<String>,
}

/// A version's membership of a branch, with whether it is the branch's latest version.
#[derive(Clone, Debug, PartialEq, Eq, sqlx::FromRow)]
pub struct BranchVersion {
    pub branch_name: String,
    pub created_at: String,
    pub latest: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, sqlx::FromRow)]
pub struct Tag {
    pub name: String,
    pub created_at: String,
}

macro_rules! version_query {
    ($where:literal) => {
        concat!(
            "SELECT v.id, v.pacticipant_id, p.name AS pacticipant_name, v.number, v.build_url, v.created_at, v.updated_at
             FROM versions v JOIN pacticipants p ON p.id = v.pacticipant_id ",
            $where
        )
    };
}

impl Db {
    pub async fn find_version(
        &self,
        pacticipant: &str,
        number: &str,
    ) -> sqlx::Result<Option<Version>> {
        sqlx::query_as(version_query!("WHERE p.name = ? AND v.number = ?"))
            .bind(pacticipant)
            .bind(number)
            .fetch_optional(&self.pool)
            .await
    }

    pub async fn find_version_by_id(&self, id: i64) -> sqlx::Result<Option<Version>> {
        sqlx::query_as(version_query!("WHERE v.id = ?"))
            .bind(id)
            .fetch_optional(&self.pool)
            .await
    }

    /// Newest first.
    pub async fn list_versions(&self, pacticipant_id: i64) -> sqlx::Result<Vec<Version>> {
        sqlx::query_as(version_query!(
            "WHERE v.pacticipant_id = ? ORDER BY v.id DESC"
        ))
        .bind(pacticipant_id)
        .fetch_all(&self.pool)
        .await
    }

    pub async fn latest_version(&self, pacticipant_id: i64) -> sqlx::Result<Option<Version>> {
        sqlx::query_as(version_query!(
            "WHERE v.pacticipant_id = ? ORDER BY v.id DESC LIMIT 1"
        ))
        .bind(pacticipant_id)
        .fetch_optional(&self.pool)
        .await
    }

    pub async fn latest_version_with_tag(
        &self,
        pacticipant_id: i64,
        tag: &str,
    ) -> sqlx::Result<Option<Version>> {
        sqlx::query_as(version_query!(
            "JOIN tags t ON t.version_id = v.id
             WHERE v.pacticipant_id = ? AND t.name = ? ORDER BY v.id DESC LIMIT 1"
        ))
        .bind(pacticipant_id)
        .bind(tag)
        .fetch_optional(&self.pool)
        .await
    }

    /// Every version carrying the tag, newest first (a `q[][tag]` selector without `latest`).
    pub async fn versions_with_tag(
        &self,
        pacticipant_id: i64,
        tag: &str,
    ) -> sqlx::Result<Vec<Version>> {
        sqlx::query_as(version_query!(
            "JOIN tags t ON t.version_id = v.id
             WHERE v.pacticipant_id = ? AND t.name = ? ORDER BY v.id DESC"
        ))
        .bind(pacticipant_id)
        .bind(tag)
        .fetch_all(&self.pool)
        .await
    }

    /// Every version in the branch, newest first (a `q[][branch]` selector without `latest`).
    pub async fn versions_in_branch(
        &self,
        pacticipant_id: i64,
        branch: &str,
    ) -> sqlx::Result<Vec<Version>> {
        sqlx::query_as(version_query!(
            "JOIN branch_versions bv ON bv.version_id = v.id
             JOIN branches b ON b.id = bv.branch_id
             WHERE v.pacticipant_id = ? AND b.name = ? ORDER BY bv.id DESC"
        ))
        .bind(pacticipant_id)
        .bind(branch)
        .fetch_all(&self.pool)
        .await
    }

    pub async fn latest_version_in_branch(
        &self,
        pacticipant_id: i64,
        branch: &str,
    ) -> sqlx::Result<Option<Version>> {
        sqlx::query_as(version_query!(
            "JOIN branch_versions bv ON bv.version_id = v.id
             JOIN branches b ON b.id = bv.branch_id
             WHERE v.pacticipant_id = ? AND b.name = ? ORDER BY bv.id DESC LIMIT 1"
        ))
        .bind(pacticipant_id)
        .bind(branch)
        .fetch_optional(&self.pool)
        .await
    }

    /// Returns the version and whether this call created it. The insert ignores a
    /// conflict, so concurrent creates of the same number both succeed.
    pub async fn find_or_create_version(
        &self,
        pacticipant_id: i64,
        number: &str,
    ) -> sqlx::Result<(Version, bool)> {
        let inserted: Option<i64> = sqlx::query_scalar(
            "INSERT INTO versions (pacticipant_id, number, created_at) VALUES (?, ?, ?)
             ON CONFLICT(pacticipant_id, number) DO NOTHING RETURNING id",
        )
        .bind(pacticipant_id)
        .bind(number)
        .bind(now())
        .fetch_optional(&self.pool)
        .await?;
        let version: Version = sqlx::query_as(version_query!(
            "WHERE v.pacticipant_id = ? AND v.number = ?"
        ))
        .bind(pacticipant_id)
        .bind(number)
        .fetch_one(&self.pool)
        .await?;
        Ok((version, inserted.is_some()))
    }

    /// Sets `updated_at` and, when given, the build URL (`Some(None)` clears it).
    pub async fn touch_version(
        &self,
        id: i64,
        build_url: Option<Option<&str>>,
    ) -> sqlx::Result<Version> {
        sqlx::query_as(
            "UPDATE versions SET
                build_url  = CASE WHEN ?1 THEN ?2 ELSE build_url END,
                updated_at = ?3
             WHERE id = ?4
             RETURNING id, pacticipant_id,
                       (SELECT name FROM pacticipants WHERE id = versions.pacticipant_id) AS pacticipant_name,
                       number, build_url, created_at, updated_at",
        )
        .bind(build_url.is_some())
        .bind(build_url.flatten())
        .bind(now())
        .bind(id)
        .fetch_one(&self.pool)
        .await
    }

    pub async fn delete_version(&self, id: i64) -> sqlx::Result<bool> {
        let r = sqlx::query("DELETE FROM versions WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(r.rows_affected() > 0)
    }

    // --- branches ---------------------------------------------------------------------

    pub async fn find_branch(
        &self,
        pacticipant_id: i64,
        name: &str,
    ) -> sqlx::Result<Option<Branch>> {
        sqlx::query_as("SELECT id, pacticipant_id, name, created_at, updated_at FROM branches WHERE pacticipant_id = ? AND name = ?")
            .bind(pacticipant_id)
            .bind(name)
            .fetch_optional(&self.pool)
            .await
    }

    /// Most recently updated first, then by name (the Ruby broker's order, verified on
    /// two captures; the tie-break is assumed).
    pub async fn list_branches(&self, pacticipant_id: i64) -> sqlx::Result<Vec<Branch>> {
        sqlx::query_as(
            "SELECT id, pacticipant_id, name, created_at, updated_at FROM branches WHERE pacticipant_id = ?
             ORDER BY COALESCE(updated_at, created_at) DESC, name",
        )
            .bind(pacticipant_id)
            .fetch_all(&self.pool)
            .await
    }

    pub async fn find_or_create_branch(
        &self,
        pacticipant_id: i64,
        name: &str,
    ) -> sqlx::Result<(Branch, bool)> {
        let inserted: Option<i64> = sqlx::query_scalar(
            "INSERT INTO branches (pacticipant_id, name, created_at) VALUES (?, ?, ?)
             ON CONFLICT(pacticipant_id, name) DO NOTHING RETURNING id",
        )
        .bind(pacticipant_id)
        .bind(name)
        .bind(now())
        .fetch_optional(&self.pool)
        .await?;
        let branch = self
            .find_branch(pacticipant_id, name)
            .await?
            .expect("just inserted or found");
        Ok((branch, inserted.is_some()))
    }

    pub async fn delete_branch(&self, id: i64) -> sqlx::Result<bool> {
        let r = sqlx::query("DELETE FROM branches WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(r.rows_affected() > 0)
    }

    /// Adds the version to the branch. Returns the membership's creation time and
    /// whether it was just created; an existing branch gets `updated_at` set.
    pub async fn find_or_create_branch_version(
        &self,
        branch_id: i64,
        version_id: i64,
    ) -> sqlx::Result<(String, bool)> {
        let inserted: Option<String> = sqlx::query_scalar(
            "INSERT INTO branch_versions (branch_id, version_id, created_at) VALUES (?, ?, ?)
             ON CONFLICT(branch_id, version_id) DO NOTHING RETURNING created_at",
        )
        .bind(branch_id)
        .bind(version_id)
        .bind(now())
        .fetch_optional(&self.pool)
        .await?;
        match inserted {
            Some(created_at) => {
                sqlx::query("UPDATE branches SET updated_at = ? WHERE id = ?")
                    .bind(now())
                    .bind(branch_id)
                    .execute(&self.pool)
                    .await?;
                Ok((created_at, true))
            }
            None => {
                let created_at: String = sqlx::query_scalar(
                    "SELECT created_at FROM branch_versions WHERE branch_id = ? AND version_id = ?",
                )
                .bind(branch_id)
                .bind(version_id)
                .fetch_one(&self.pool)
                .await?;
                Ok((created_at, false))
            }
        }
    }

    pub async fn find_branch_version(
        &self,
        branch_id: i64,
        version_id: i64,
    ) -> sqlx::Result<Option<String>> {
        sqlx::query_scalar(
            "SELECT created_at FROM branch_versions WHERE branch_id = ? AND version_id = ?",
        )
        .bind(branch_id)
        .bind(version_id)
        .fetch_optional(&self.pool)
        .await
    }

    pub async fn delete_branch_version(
        &self,
        branch_id: i64,
        version_id: i64,
    ) -> sqlx::Result<bool> {
        let r = sqlx::query("DELETE FROM branch_versions WHERE branch_id = ? AND version_id = ?")
            .bind(branch_id)
            .bind(version_id)
            .execute(&self.pool)
            .await?;
        Ok(r.rows_affected() > 0)
    }

    /// The branches a version belongs to, in the order it joined them. `latest` means the
    /// version was the last one added to that branch (the Ruby broker's branch head),
    /// which is not always the highest version order (verified).
    pub async fn branch_versions_for_version(
        &self,
        version_id: i64,
    ) -> sqlx::Result<Vec<BranchVersion>> {
        sqlx::query_as(
            "SELECT b.name AS branch_name, bv.created_at,
                    (bv.id = (SELECT MAX(id) FROM branch_versions WHERE branch_id = b.id)) AS latest
             FROM branch_versions bv JOIN branches b ON b.id = bv.branch_id
             WHERE bv.version_id = ? ORDER BY bv.id",
        )
        .bind(version_id)
        .fetch_all(&self.pool)
        .await
    }

    // --- tags -------------------------------------------------------------------------

    pub async fn find_tag(&self, version_id: i64, name: &str) -> sqlx::Result<Option<Tag>> {
        sqlx::query_as("SELECT name, created_at FROM tags WHERE version_id = ? AND name = ?")
            .bind(version_id)
            .bind(name)
            .fetch_optional(&self.pool)
            .await
    }

    /// In creation order.
    pub async fn tags_for_version(&self, version_id: i64) -> sqlx::Result<Vec<Tag>> {
        sqlx::query_as("SELECT name, created_at FROM tags WHERE version_id = ? ORDER BY id")
            .bind(version_id)
            .fetch_all(&self.pool)
            .await
    }

    pub async fn find_or_create_tag(
        &self,
        version_id: i64,
        name: &str,
    ) -> sqlx::Result<(Tag, bool)> {
        let inserted: Option<String> = sqlx::query_scalar(
            "INSERT INTO tags (version_id, name, created_at) VALUES (?, ?, ?)
             ON CONFLICT(version_id, name) DO NOTHING RETURNING created_at",
        )
        .bind(version_id)
        .bind(name)
        .bind(now())
        .fetch_optional(&self.pool)
        .await?;
        let tag = self
            .find_tag(version_id, name)
            .await?
            .expect("just inserted or found");
        Ok((tag, inserted.is_some()))
    }

    pub async fn delete_tag(&self, version_id: i64, name: &str) -> sqlx::Result<bool> {
        let r = sqlx::query("DELETE FROM tags WHERE version_id = ? AND name = ?")
            .bind(version_id)
            .bind(name)
            .execute(&self.pool)
            .await?;
        Ok(r.rows_affected() > 0)
    }

    /// Provider names this consumer version has published pacts for, ordered by name.
    pub async fn providers_for_consumer_version(
        &self,
        version_id: i64,
    ) -> sqlx::Result<Vec<String>> {
        sqlx::query_scalar(
            "SELECT p.name FROM pact_publications pp JOIN pacticipants p ON p.id = pp.provider_id
             WHERE pp.consumer_version_id = ? ORDER BY p.name",
        )
        .bind(version_id)
        .fetch_all(&self.pool)
        .await
    }

    /// Sets the main branch only when none is set. Returns whether it changed.
    pub async fn set_main_branch_if_unset(
        &self,
        pacticipant_id: i64,
        branch: &str,
    ) -> sqlx::Result<bool> {
        let r = sqlx::query(
            "UPDATE pacticipants SET main_branch = ?, updated_at = ? WHERE id = ? AND main_branch IS NULL",
        )
        .bind(branch)
        .bind(now())
        .bind(pacticipant_id)
        .execute(&self.pool)
        .await?;
        Ok(r.rows_affected() > 0)
    }
}
