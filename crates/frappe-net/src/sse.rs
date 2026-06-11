use actix_web::{get, Responder, HttpResponse, web};
use crate::middleware::tenant::AppState;

#[get("/api/events")]
pub async fn sse_events(state: web::Data<AppState>) -> impl Responder {
    let rx = state.broadcaster.subscribe();
    
    // Send initial connection event, then stream broadcasted events
    let stream = futures_util::stream::unfold((rx, true), |(mut rx, is_first)| async move {
        if is_first {
            let chunk = "data: connected\n\n";
            return Some((Ok::<_, actix_web::Error>(actix_web::web::Bytes::from(chunk)), (rx, false)));
        }

        match rx.recv().await {
            Ok(msg) => {
                let chunk = format!("data: {}\n\n", msg);
                Some((Ok::<_, actix_web::Error>(actix_web::web::Bytes::from(chunk)), (rx, false)))
            }
            Err(_) => None,
        }
    });

    HttpResponse::Ok()
        .insert_header(("content-type", "text/event-stream"))
        .insert_header(("cache-control", "no-cache"))
        .insert_header(("connection", "keep-alive"))
        .streaming(stream)
}
