use super::{Db, now};

#[derive(Clone, Debug, PartialEq, Eq, sqlx::FromRow)]
pub struct Pacticipant {
    pub id: i64,
    pub name: String,
    pub display_name: Option<String>,
    pub repository_url: Option<String>,
    pub main_branch: Option<String>,
    pub created_at: String,
    pub updated_at: Option<String>,
}

impl Db {
    pub async fn find_pacticipant(&self, name: &str) -> sqlx::Result<Option<Pacticipant>> {
        sqlx::query_as(
            "SELECT id, name, display_name, repository_url, main_branch, created_at, updated_at
             FROM pacticipants WHERE name = ?",
        )
        .bind(name)
        .fetch_optional(&self.pool)
        .await
    }

    /// Ordered case-insensitively by name, as the Ruby broker lists them.
    pub async fn list_pacticipants(&self) -> sqlx::Result<Vec<Pacticipant>> {
        sqlx::query_as(
            "SELECT id, name, display_name, repository_url, main_branch, created_at, updated_at
             FROM pacticipants ORDER BY name COLLATE NOCASE",
        )
        .fetch_all(&self.pool)
        .await
    }

    /// Create-or-update in one statement. On create, `display_name` is stored as given
    /// (the caller supplies the default); on update only the provided columns change.
    /// Returns the row and whether it was created.
    pub async fn upsert_pacticipant(
        &self,
        name: &str,
        display_name: Option<Option<&str>>,
        default_display_name: &str,
        repository_url: Option<Option<&str>>,
        main_branch: Option<Option<&str>>,
    ) -> sqlx::Result<(Pacticipant, bool)> {
        let row: (Pacticipant, bool) = sqlx::query_as::<_, PacticipantWithCreated>(
            "INSERT INTO pacticipants (name, display_name, repository_url, main_branch, created_at)
             VALUES (?1, CASE WHEN ?2 THEN ?3 ELSE ?4 END, ?6, ?8, ?9)
             ON CONFLICT(name) DO UPDATE SET
                display_name   = CASE WHEN ?2 THEN ?3 ELSE display_name END,
                repository_url = CASE WHEN ?5 THEN ?6 ELSE repository_url END,
                main_branch    = CASE WHEN ?7 THEN ?8 ELSE main_branch END,
                updated_at     = ?9
             RETURNING id, name, display_name, repository_url, main_branch, created_at, updated_at,
                       (updated_at IS NULL) AS created",
        )
        .bind(name)
        .bind(display_name.is_some())
        .bind(display_name.flatten())
        .bind(default_display_name)
        .bind(repository_url.is_some())
        .bind(repository_url.flatten())
        .bind(main_branch.is_some())
        .bind(main_branch.flatten())
        .bind(now())
        .fetch_one(&self.pool)
        .await?
        .into();
        Ok(row)
    }

    /// Update only. `None` when no such pacticipant exists.
    pub async fn update_pacticipant(
        &self,
        name: &str,
        display_name: Option<Option<&str>>,
        repository_url: Option<Option<&str>>,
        main_branch: Option<Option<&str>>,
    ) -> sqlx::Result<Option<Pacticipant>> {
        sqlx::query_as(
            "UPDATE pacticipants SET
                display_name   = CASE WHEN ?1 THEN ?2 ELSE display_name END,
                repository_url = CASE WHEN ?3 THEN ?4 ELSE repository_url END,
                main_branch    = CASE WHEN ?5 THEN ?6 ELSE main_branch END,
                updated_at     = ?7
             WHERE name = ?8
             RETURNING id, name, display_name, repository_url, main_branch, created_at, updated_at",
        )
        .bind(display_name.is_some())
        .bind(display_name.flatten())
        .bind(repository_url.is_some())
        .bind(repository_url.flatten())
        .bind(main_branch.is_some())
        .bind(main_branch.flatten())
        .bind(now())
        .bind(name)
        .fetch_optional(&self.pool)
        .await
    }

    /// Returns whether a row was deleted.
    pub async fn delete_pacticipant(&self, name: &str) -> sqlx::Result<bool> {
        let result = sqlx::query("DELETE FROM pacticipants WHERE name = ?")
            .bind(name)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }
}

#[derive(sqlx::FromRow)]
struct PacticipantWithCreated {
    #[sqlx(flatten)]
    pacticipant: Pacticipant,
    created: bool,
}

impl From<PacticipantWithCreated> for (Pacticipant, bool) {
    fn from(r: PacticipantWithCreated) -> Self {
        (r.pacticipant, r.created)
    }
}
