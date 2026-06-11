use std::time::Duration;
use tokio::time::sleep;
use surrealdb::Surreal;
use surrealdb::engine::any::Any;

#[derive(serde::Deserialize, serde::Serialize, Debug)]
pub struct WebhookQueueItem {
    pub id: surrealdb::sql::Thing,
    pub url: String,
    pub payload: String,
    pub status: String, // "Pending", "Completed", "Failed"
    pub retry_count: i32,
}

pub struct WebhookWorker;

impl WebhookWorker {
    pub async fn process_queue(db: Surreal<Any>) {
        let client = reqwest::Client::new();

        loop {
            sleep(Duration::from_secs(5)).await;

            // Fetch pending webhooks
            let mut res = match db.query("SELECT * FROM tabWebhookQueue WHERE status = 'Pending' LIMIT 10;").await {
                Ok(r) => r,
                Err(e) => {
                    log::error!("Failed to query webhook queue: {:?}", e);
                    continue;
                }
            };

            let items: Vec<WebhookQueueItem> = res.take(0).unwrap_or_default();

            for mut item in items {
                log::info!("Processing webhook to: {}", item.url);

                match client.post(&item.url)
                    .header("content-type", "application/json")
                    .body(item.payload.clone())
                    .send()
                    .await
                {
                    Ok(resp) if resp.status().is_success() => {
                        log::info!("Webhook delivered successfully to {}", item.url);
                        let _ = db.query("UPDATE $id SET status = 'Completed';")
                            .bind(("id", item.id.clone()))
                            .await;
                    }
                    Ok(resp) => {
                        log::warn!("Webhook failed with status: {:?}", resp.status());
                        Self::handle_failure(&db, &mut item).await;
                    }
                    Err(e) => {
                        log::error!("Webhook error: {:?}", e);
                        Self::handle_failure(&db, &mut item).await;
                    }
                }
            }
        }
    }

    async fn handle_failure(db: &Surreal<Any>, item: &mut WebhookQueueItem) {
        item.retry_count += 1;
        if item.retry_count >= 3 {
            log::error!("Webhook to {} reached max retries. Marking as Failed.", item.url);
            let _ = db.query("UPDATE $id SET status = 'Failed', retry_count = $retry;")
                .bind(("id", item.id.clone()))
                .bind(("retry", item.retry_count))
                .await;
        } else {
            let _ = db.query("UPDATE $id SET retry_count = $retry;")
                .bind(("id", item.id.clone()))
                .bind(("retry", item.retry_count))
                .await;
        }
    }
}
