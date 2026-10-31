use super::{Db, now};

#[derive(Clone, Debug, PartialEq, Eq, sqlx::FromRow)]
pub struct Pacticipant {
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
            "SELECT name, display_name, repository_url, main_branch, created_at, updated_at
             FROM pacticipants WHERE name = ?",
        )
        .bind(name)
        .fetch_optional(&self.pool)
        .await
    }

    /// Ordered case-insensitively by name, as the Ruby broker lists them.
    pub async fn list_pacticipants(&self) -> sqlx::Result<Vec<Pacticipant>> {
        sqlx::query_as(
            "SELECT name, display_name, repository_url, main_branch, created_at, updated_at
             FROM pacticipants ORDER BY name COLLATE NOCASE",
        )
        .fetch_all(&self.pool)
        .await
    }

    pub async fn insert_pacticipant(
        &self,
        name: &str,
        display_name: Option<&str>,
        repository_url: Option<&str>,
        main_branch: Option<&str>,
    ) -> sqlx::Result<Pacticipant> {
        sqlx::query(
            "INSERT INTO pacticipants (name, display_name, repository_url, main_branch, created_at)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(name)
        .bind(display_name)
        .bind(repository_url)
        .bind(main_branch)
        .bind(now())
        .execute(&self.pool)
        .await?;
        Ok(self.find_pacticipant(name).await?.expect("just inserted"))
    }

    /// Overwrites every column given; `None` leaves the column as it is.
    pub async fn update_pacticipant(
        &self,
        name: &str,
        display_name: Option<Option<&str>>,
        repository_url: Option<Option<&str>>,
        main_branch: Option<Option<&str>>,
    ) -> sqlx::Result<Pacticipant> {
        sqlx::query(
            "UPDATE pacticipants SET
                display_name   = CASE WHEN ?1 THEN ?2 ELSE display_name END,
                repository_url = CASE WHEN ?3 THEN ?4 ELSE repository_url END,
                main_branch    = CASE WHEN ?5 THEN ?6 ELSE main_branch END,
                updated_at     = ?7
             WHERE name = ?8",
        )
        .bind(display_name.is_some())
        .bind(display_name.flatten())
        .bind(repository_url.is_some())
        .bind(repository_url.flatten())
        .bind(main_branch.is_some())
        .bind(main_branch.flatten())
        .bind(now())
        .bind(name)
        .execute(&self.pool)
        .await?;
        Ok(self.find_pacticipant(name).await?.expect("just updated"))
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
