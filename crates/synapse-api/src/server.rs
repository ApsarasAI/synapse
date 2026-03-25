use std::net::SocketAddr;

use axum::{extract::Json, routing::get, routing::post, Router};
use synapse_core::{ExecuteRequest, ExecuteResponse};

pub fn router() -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/execute", post(execute))
}

pub async fn serve(listen: SocketAddr) -> Result<(), std::io::Error> {
    let listener = tokio::net::TcpListener::bind(listen).await?;
    axum::serve(listener, router()).await
}

async fn health() -> &'static str {
    "ok"
}

async fn execute(Json(req): Json<ExecuteRequest>) -> Json<ExecuteResponse> {
    let mut resp = ExecuteResponse::mock_ok();
    if req.code.trim().is_empty() {
        resp.stderr = "code cannot be empty".to_string();
        resp.exit_code = -1;
    } else {
        resp.stdout = format!("language={} initialized", req.language);
    }
    Json(resp)
}
