use actix_web::{web, App, HttpServer};
use surrealdb::engine::any::connect;
use tokio::task;

use frappe_net::middleware::tenant::{AppState, TenantResolver};
use frappe_net::h3_server::H3Server;
use frappe_net::webhook_worker::WebhookWorker;
use frappe_net::routes;


#[actix_web::main]
async fn main() -> std::io::Result<()> {
    env_logger::init_from_env(env_logger::Env::new().default_filter_or("info"));
    
    // Connect to SurrealDB (in-memory for development/testing by default)
    let db = connect("mem://").await.expect("Failed to connect to SurrealDB");
    
    // Create shared SSE broadcast channel
    let (broadcaster, _) = tokio::sync::broadcast::channel(100);
    
    let app_state = web::Data::new(AppState { db: db.clone(), broadcaster });

    // Start Webhook background worker
    let db_for_worker = db.clone();
    task::spawn(async move {
        log::info!("Starting background webhook queue processor worker");
        WebhookWorker::process_queue(db_for_worker).await;
    });

    // Start QUIC HTTP/3 server in a background task
    let quic_addr = "127.0.0.1:8080".parse().unwrap();
    let quic_server = H3Server::new(quic_addr).await.expect("Failed to bind UDP socket");
    
    task::spawn(async move {
        log::info!("Starting QUIC HTTP/3 Server on UDP 8080");
        if let Err(e) = quic_server.run_loop().await {
            log::error!("QUIC Server error: {:?}", e);
        }
    });

    // Start Actix HTTP/2 Server
    log::info!("Starting Actix HTTP Server on TCP 8080");
    HttpServer::new(move || {
        App::new()
            .app_data(app_state.clone())
            .wrap(TenantResolver)
            .configure(routes::config)
    })
    .bind(("127.0.0.1", 8080))?
    .run()
    .await
}
