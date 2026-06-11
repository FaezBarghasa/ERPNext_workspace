use actix_web::{test, web, App};
use frappe_net::middleware::tenant::{AppState, TenantResolver};
use frappe_net::routes;
use surrealdb::engine::any::connect;
use std::time::Instant;

#[actix_web::test]
async fn test_tenant_routing_benchmark() {
    let db = connect("mem://").await.expect("Failed to connect");
    let app_state = web::Data::new(AppState { db: db.clone() });

    db.use_ns("frappe_cloud").use_db("tenant1").await.unwrap();
    db.query("CREATE Customer CONTENT { name: 'Tenant1Customer' };").await.unwrap();

    let app = test::init_service(
        App::new()
            .app_data(app_state.clone())
            .wrap(TenantResolver)
            .configure(routes::config)
    ).await;

    let start = Instant::now();
    for _ in 0..10_000 {
        let req = test::TestRequest::get()
            .uri("/api/resource/Customer/Tenant1Customer")
            .insert_header(("Host", "tenant1.localhost"))
            .to_request();
        let _ = test::call_service(&app, req).await;
    }
    let duration = start.elapsed();
    
    // Using a basic assert or print for benchmark since actix-web::test doesn't use standard bench framework natively here
    println!("10,000 lookups completed in {:?}", duration);
    let avg = duration.as_micros() as f64 / 10_000.0;
    println!("Average lookup speed: {} microseconds", avg);
    
    // We expect very fast times, although full HTTP request cycle is measured here, not just lookup
}
