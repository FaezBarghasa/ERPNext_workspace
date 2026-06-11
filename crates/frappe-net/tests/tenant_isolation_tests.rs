use actix_web::{test, web, App};
use frappe_net::middleware::tenant::{AppState, TenantResolver};
use frappe_net::routes;
use surrealdb::engine::any::connect;

#[actix_web::test]
async fn test_tenant_isolation() {
    let db = connect("mem://").await.expect("Failed to connect to SurrealDB");
    let (tx, _) = tokio::sync::broadcast::channel(100);
    let app_state = web::Data::new(AppState { db: db.clone(), broadcaster: tx });

    // Setup mock databases
    db.use_ns("frappe_cloud").use_db("tenant1").await.unwrap();
    db.query("CREATE Customer CONTENT { name: 'Tenant1Customer' };").await.unwrap();

    db.use_ns("frappe_cloud").use_db("tenant2").await.unwrap();
    db.query("CREATE Customer CONTENT { name: 'Tenant2Customer' };").await.unwrap();

    let app = test::init_service(
        App::new()
            .app_data(app_state.clone())
            .wrap(TenantResolver)
            .configure(routes::config)
    ).await;

    // Test Tenant 1
    let req1 = test::TestRequest::get()
        .uri("/api/resource/Customer/Tenant1Customer")
        .insert_header(("Host", "tenant1.localhost"))
        .to_request();
    let resp1 = test::call_service(&app, req1).await;
    assert!(resp1.status().is_success());
    let body1 = test::read_body(resp1).await;
    assert!(std::str::from_utf8(&body1).unwrap().contains("Tenant1Customer"));

    // Test Tenant 2
    let req2 = test::TestRequest::get()
        .uri("/api/resource/Customer/Tenant2Customer")
        .insert_header(("Host", "tenant2.localhost"))
        .to_request();
    let resp2 = test::call_service(&app, req2).await;
    assert!(resp2.status().is_success());

    // Test cross-tenant isolation (should fail with 404)
    let req3 = test::TestRequest::get()
        .uri("/api/resource/Customer/Tenant2Customer")
        .insert_header(("Host", "tenant1.localhost"))
        .to_request();
    let resp3 = test::call_service(&app, req3).await;
    assert_eq!(resp3.status(), actix_web::http::StatusCode::NOT_FOUND);
}
