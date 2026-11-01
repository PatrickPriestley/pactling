//! Webhooks, triggered webhooks and their executions.
use super::{Db, now};

#[derive(Clone, Debug, PartialEq, Eq, sqlx::FromRow)]
pub struct Webhook {
    pub id: i64,
    pub uuid: String,
    pub description: Option<String>,
    pub consumer_id: Option<i64>,
    pub consumer_name: Option<String>,
    pub consumer_label: Option<String>,
    pub provider_id: Option<i64>,
    pub provider_name: Option<String>,
    pub provider_label: Option<String>,
    pub enabled: bool,
    pub method: String,
    pub url: String,
    /// JSON object text, header names as given.
    pub headers: Option<String>,
    /// JSON text of the body value (an object or a string).
    pub body: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub created_at: String,
    pub updated_at: Option<String>,
    #[sqlx(skip)]
    pub events: Vec<String>,
}

/// The columns written on create and update.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WebhookFields {
    pub description: Option<String>,
    pub consumer_id: Option<i64>,
    pub consumer_label: Option<String>,
    pub provider_id: Option<i64>,
    pub provider_label: Option<String>,
    pub enabled: bool,
    pub method: String,
    pub url: String,
    pub headers: Option<String>,
    pub body: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub events: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, sqlx::FromRow)]
pub struct TriggeredWebhook {
    pub id: i64,
    pub uuid: String,
    pub webhook_id: Option<i64>,
    pub webhook_uuid: String,
    pub consumer_id: i64,
    pub consumer_name: String,
    pub provider_id: i64,
    pub provider_name: String,
    pub pact_publication_id: Option<i64>,
    pub verification_id: Option<i64>,
    pub trigger_type: String,
    pub event_name: String,
    pub event_context: String,
    pub status: String,
    pub next_attempt_at: Option<String>,
    pub created_at: String,
    pub updated_at: Option<String>,
    /// Executions so far (attempts made).
    #[sqlx(skip)]
    pub executions: Vec<WebhookExecution>,
}

#[derive(Clone, Debug, PartialEq, Eq, sqlx::FromRow)]
pub struct WebhookExecution {
    pub id: i64,
    pub triggered_webhook_id: i64,
    pub success: bool,
    pub logs: String,
    pub created_at: String,
}

macro_rules! webhook_query {
    ($tail:literal) => {
        concat!(
            "SELECT w.id, w.uuid, w.description, w.consumer_id, c.name AS consumer_name, w.consumer_label,
                    w.provider_id, p.name AS provider_name, w.provider_label, w.enabled, w.method, w.url,
                    w.headers, w.body, w.username, w.password, w.created_at, w.updated_at
             FROM webhooks w
             LEFT JOIN pacticipants c ON c.id = w.consumer_id
             LEFT JOIN pacticipants p ON p.id = w.provider_id ",
            $tail
        )
    };
}

macro_rules! triggered_query {
    ($tail:literal) => {
        concat!(
            "SELECT t.id, t.uuid, t.webhook_id, t.webhook_uuid, t.consumer_id, c.name AS consumer_name,
                    t.provider_id, p.name AS provider_name, t.pact_publication_id, t.verification_id,
                    t.trigger_type, t.event_name, t.event_context, t.status, t.next_attempt_at,
                    t.created_at, t.updated_at
             FROM triggered_webhooks t
             JOIN pacticipants c ON c.id = t.consumer_id
             JOIN pacticipants p ON p.id = t.provider_id ",
            $tail
        )
    };
}

impl Db {
    // --- webhooks ---------------------------------------------------------------------

    async fn attach_events(&self, mut webhooks: Vec<Webhook>) -> sqlx::Result<Vec<Webhook>> {
        for w in &mut webhooks {
            w.events = sqlx::query_scalar(
                "SELECT name FROM webhook_events WHERE webhook_id = ? ORDER BY id",
            )
            .bind(w.id)
            .fetch_all(&self.pool)
            .await?;
        }
        Ok(webhooks)
    }

    pub async fn create_webhook(&self, uuid: &str, f: &WebhookFields) -> sqlx::Result<Webhook> {
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO webhooks (uuid, description, consumer_id, provider_id, consumer_label, provider_label, enabled,
                                   method, url, headers, body, username, password, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
        )
        .bind(uuid)
        .bind(&f.description)
        .bind(f.consumer_id)
        .bind(f.provider_id)
        .bind(&f.consumer_label)
        .bind(&f.provider_label)
        .bind(f.enabled)
        .bind(&f.method)
        .bind(&f.url)
        .bind(&f.headers)
        .bind(&f.body)
        .bind(&f.username)
        .bind(&f.password)
        .bind(now())
        .fetch_one(&self.pool)
        .await?;
        self.replace_events(id, &f.events).await?;
        Ok(self.find_webhook_by_id(id).await?.expect("just created"))
    }

    pub async fn update_webhook(&self, id: i64, f: &WebhookFields) -> sqlx::Result<Webhook> {
        sqlx::query(
            "UPDATE webhooks SET description = ?, consumer_id = ?, provider_id = ?, consumer_label = ?, provider_label = ?,
                    enabled = ?, method = ?, url = ?, headers = ?, body = ?, username = ?, password = ?, updated_at = ?
             WHERE id = ?",
        )
        .bind(&f.description)
        .bind(f.consumer_id)
        .bind(f.provider_id)
        .bind(&f.consumer_label)
        .bind(&f.provider_label)
        .bind(f.enabled)
        .bind(&f.method)
        .bind(&f.url)
        .bind(&f.headers)
        .bind(&f.body)
        .bind(&f.username)
        .bind(&f.password)
        .bind(now())
        .bind(id)
        .execute(&self.pool)
        .await?;
        self.replace_events(id, &f.events).await?;
        Ok(self.find_webhook_by_id(id).await?.expect("exists"))
    }

    async fn replace_events(&self, webhook_id: i64, events: &[String]) -> sqlx::Result<()> {
        sqlx::query("DELETE FROM webhook_events WHERE webhook_id = ?")
            .bind(webhook_id)
            .execute(&self.pool)
            .await?;
        for name in events {
            sqlx::query("INSERT OR IGNORE INTO webhook_events (webhook_id, name) VALUES (?, ?)")
                .bind(webhook_id)
                .bind(name)
                .execute(&self.pool)
                .await?;
        }
        Ok(())
    }

    pub async fn find_webhook_by_id(&self, id: i64) -> sqlx::Result<Option<Webhook>> {
        let w: Option<Webhook> = sqlx::query_as(webhook_query!("WHERE w.id = ?"))
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(self.attach_events(w.into_iter().collect()).await?.pop())
    }

    pub async fn find_webhook_by_uuid(&self, uuid: &str) -> sqlx::Result<Option<Webhook>> {
        let w: Option<Webhook> = sqlx::query_as(webhook_query!("WHERE w.uuid = ?"))
            .bind(uuid)
            .fetch_optional(&self.pool)
            .await?;
        Ok(self.attach_events(w.into_iter().collect()).await?.pop())
    }

    /// Every webhook, oldest first.
    pub async fn list_webhooks(&self) -> sqlx::Result<Vec<Webhook>> {
        let ws = sqlx::query_as(webhook_query!("ORDER BY w.id"))
            .fetch_all(&self.pool)
            .await?;
        self.attach_events(ws).await
    }

    /// Webhooks whose scope is exactly this consumer and this provider, either of which
    /// may be "none" (verified: `/webhooks/provider/P` lists only provider-only webhooks).
    pub async fn webhooks_with_scope(
        &self,
        consumer_id: Option<i64>,
        provider_id: Option<i64>,
    ) -> sqlx::Result<Vec<Webhook>> {
        let ws = sqlx::query_as(webhook_query!(
            "WHERE w.consumer_id IS ? AND w.provider_id IS ? ORDER BY w.id"
        ))
        .bind(consumer_id)
        .bind(provider_id)
        .fetch_all(&self.pool)
        .await?;
        self.attach_events(ws).await
    }

    /// Enabled webhooks that apply to a pact (scoped to its consumer, its provider, both,
    /// or neither) and listen to the event. Verified: the Ruby broker's
    /// `find_webhooks_to_trigger`.
    pub async fn webhooks_to_trigger(
        &self,
        consumer_id: i64,
        provider_id: i64,
        event: &str,
    ) -> sqlx::Result<Vec<Webhook>> {
        let ws = sqlx::query_as(webhook_query!(
            "JOIN webhook_events e ON e.webhook_id = w.id
             WHERE w.enabled = 1 AND e.name = ?
               AND (w.consumer_id IS NULL OR w.consumer_id = ?)
               AND (w.provider_id IS NULL OR w.provider_id = ?)
               AND w.consumer_label IS NULL AND w.provider_label IS NULL
             ORDER BY w.id"
        ))
        .bind(event)
        .bind(consumer_id)
        .bind(provider_id)
        .fetch_all(&self.pool)
        .await?;
        self.attach_events(ws).await
    }

    pub async fn delete_webhook(&self, id: i64) -> sqlx::Result<()> {
        sqlx::query("DELETE FROM webhooks WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // --- triggered webhooks -----------------------------------------------------------

    async fn attach_executions(
        &self,
        mut rows: Vec<TriggeredWebhook>,
    ) -> sqlx::Result<Vec<TriggeredWebhook>> {
        for t in &mut rows {
            t.executions = sqlx::query_as(
                "SELECT id, triggered_webhook_id, success, logs, created_at FROM webhook_executions WHERE triggered_webhook_id = ? ORDER BY id",
            )
            .bind(t.id)
            .fetch_all(&self.pool)
            .await?;
        }
        Ok(rows)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn create_triggered_webhook(
        &self,
        uuid: &str,
        webhook: &Webhook,
        consumer_id: i64,
        provider_id: i64,
        pact_publication_id: Option<i64>,
        verification_id: Option<i64>,
        trigger_type: &str,
        event_name: &str,
        event_context: &str,
        next_attempt_at: &str,
    ) -> sqlx::Result<TriggeredWebhook> {
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO triggered_webhooks (uuid, webhook_id, webhook_uuid, consumer_id, provider_id, pact_publication_id,
                                             verification_id, trigger_type, event_name, event_context, status, next_attempt_at, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'not_run', ?, ?) RETURNING id",
        )
        .bind(uuid)
        .bind(webhook.id)
        .bind(&webhook.uuid)
        .bind(consumer_id)
        .bind(provider_id)
        .bind(pact_publication_id)
        .bind(verification_id)
        .bind(trigger_type)
        .bind(event_name)
        .bind(event_context)
        .bind(next_attempt_at)
        .bind(now())
        .fetch_one(&self.pool)
        .await?;
        Ok(self
            .find_triggered_webhook_by_id(id)
            .await?
            .expect("just created"))
    }

    pub async fn find_triggered_webhook_by_id(
        &self,
        id: i64,
    ) -> sqlx::Result<Option<TriggeredWebhook>> {
        let t: Option<TriggeredWebhook> = sqlx::query_as(triggered_query!("WHERE t.id = ?"))
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(self.attach_executions(t.into_iter().collect()).await?.pop())
    }

    pub async fn find_triggered_webhook_by_uuid(
        &self,
        uuid: &str,
    ) -> sqlx::Result<Option<TriggeredWebhook>> {
        let t: Option<TriggeredWebhook> = sqlx::query_as(triggered_query!("WHERE t.uuid = ?"))
            .bind(uuid)
            .fetch_optional(&self.pool)
            .await?;
        Ok(self.attach_executions(t.into_iter().collect()).await?.pop())
    }

    /// Every triggered webhook recorded against the publication, whatever its event
    /// (verified: `find_triggered_webhooks_for_pact` filters by pact only), newest first.
    pub async fn triggered_webhooks_for_pact_publication(
        &self,
        pact_publication_id: i64,
    ) -> sqlx::Result<Vec<TriggeredWebhook>> {
        let rows = sqlx::query_as(triggered_query!(
            "WHERE t.pact_publication_id = ? ORDER BY t.id DESC"
        ))
        .bind(pact_publication_id)
        .fetch_all(&self.pool)
        .await?;
        self.attach_executions(rows).await
    }

    pub async fn triggered_webhooks_for_verification(
        &self,
        verification_id: i64,
    ) -> sqlx::Result<Vec<TriggeredWebhook>> {
        let rows = sqlx::query_as(triggered_query!(
            "WHERE t.verification_id = ? ORDER BY t.id DESC"
        ))
        .bind(verification_id)
        .fetch_all(&self.pool)
        .await?;
        self.attach_executions(rows).await
    }

    /// The newest triggered webhook per (webhook, event) for an integration, oldest
    /// first; webhooks since deleted are left out (verified: `latest_triggered_webhooks`).
    pub async fn latest_triggered_webhooks(
        &self,
        consumer_id: i64,
        provider_id: i64,
    ) -> sqlx::Result<Vec<TriggeredWebhook>> {
        let rows = sqlx::query_as(triggered_query!(
            "WHERE t.consumer_id = ? AND t.provider_id = ? AND t.webhook_id IS NOT NULL
               AND t.id = (SELECT MAX(o.id) FROM triggered_webhooks o
                           WHERE o.consumer_id = t.consumer_id AND o.provider_id = t.provider_id
                             AND o.webhook_uuid = t.webhook_uuid AND o.event_name = t.event_name)
             ORDER BY t.id"
        ))
        .bind(consumer_id)
        .bind(provider_id)
        .fetch_all(&self.pool)
        .await?;
        self.attach_executions(rows).await
    }

    /// Triggered webhooks whose next attempt is due, oldest first.
    pub async fn due_triggered_webhooks(
        &self,
        now_rfc3339: &str,
    ) -> sqlx::Result<Vec<TriggeredWebhook>> {
        let rows = sqlx::query_as(triggered_query!(
            "WHERE t.status IN ('not_run', 'retrying') AND t.next_attempt_at IS NOT NULL AND t.next_attempt_at <= ? ORDER BY t.id"
        ))
        .bind(now_rfc3339)
        .fetch_all(&self.pool)
        .await?;
        self.attach_executions(rows).await
    }

    pub async fn record_execution(
        &self,
        triggered_webhook_id: i64,
        success: bool,
        logs: &str,
    ) -> sqlx::Result<WebhookExecution> {
        sqlx::query_as(
            "INSERT INTO webhook_executions (triggered_webhook_id, success, logs, created_at) VALUES (?, ?, ?, ?)
             RETURNING id, triggered_webhook_id, success, logs, created_at",
        )
        .bind(triggered_webhook_id)
        .bind(success)
        .bind(logs)
        .bind(now())
        .fetch_one(&self.pool)
        .await
    }

    /// Records an attempt and the triggered webhook's new status atomically.
    pub async fn finish_execution(
        &self,
        triggered_webhook_id: i64,
        success: bool,
        logs: &str,
        status: &str,
        next_attempt_at: Option<&str>,
    ) -> sqlx::Result<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("INSERT INTO webhook_executions (triggered_webhook_id, success, logs, created_at) VALUES (?, ?, ?, ?)")
            .bind(triggered_webhook_id)
            .bind(success)
            .bind(logs)
            .bind(now())
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE triggered_webhooks SET status = ?, next_attempt_at = ?, updated_at = ? WHERE id = ?")
            .bind(status)
            .bind(next_attempt_at)
            .bind(now())
            .bind(triggered_webhook_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await
    }

    pub async fn set_triggered_webhook_status(
        &self,
        id: i64,
        status: &str,
        next_attempt_at: Option<&str>,
    ) -> sqlx::Result<()> {
        sqlx::query("UPDATE triggered_webhooks SET status = ?, next_attempt_at = ?, updated_at = ? WHERE id = ?")
            .bind(status)
            .bind(next_attempt_at)
            .bind(now())
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
