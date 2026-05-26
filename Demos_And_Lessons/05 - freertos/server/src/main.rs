//! Native Rust HTTP server that ingests data from the ESP32 and exposes a
//! live web page. Built with axum + tokio. Replaces the previous Flask demo
//! so the whole stack (firmware + server) is Rust.
//!
//! Endpoints:
//!   GET  /              -> embedded HTML dashboard
//!   POST /api/data      -> the ESP32 posts {"value": <i32>} here
//!   GET  /api/history   -> JSON array of the last 100 samples
//!
//! Run with:
//!     cargo run --release
//! Then open http://localhost:5000 in your browser.

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    extract::State,
    http::StatusCode,
    response::{Html, IntoResponse, Json},
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

const HISTORY_CAPACITY: usize = 100;
const LISTEN_ADDR: &str = "0.0.0.0:5000";

#[derive(Clone)]
struct AppState {
    history: Arc<Mutex<VecDeque<Sample>>>,
}

#[derive(Clone, Serialize)]
struct Sample {
    value: i32,
    ts: String,
}

#[derive(Deserialize)]
struct IncomingData {
    value: i32,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let state = AppState {
        history: Arc::new(Mutex::new(VecDeque::with_capacity(HISTORY_CAPACITY))),
    };

    let app = Router::new()
        .route("/", get(index))
        .route("/api/data", post(ingest))
        .route("/api/history", get(history))
        .with_state(state);

    let addr: SocketAddr = LISTEN_ADDR.parse().expect("invalid LISTEN_ADDR");
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("failed to bind TCP socket");

    tracing::info!("listening on http://{}", addr);
    tracing::info!("open http://localhost:5000 in your browser");

    axum::serve(listener, app)
        .await
        .expect("server crashed");
}

async fn index() -> Html<&'static str> {
    Html(include_str!("index.html"))
}

async fn ingest(
    State(state): State<AppState>,
    Json(payload): Json<IncomingData>,
) -> impl IntoResponse {
    let sample = Sample {
        value: payload.value,
        ts: timestamp_now(),
    };

    let mut history = state.history.lock().await;
    if history.len() == HISTORY_CAPACITY {
        history.pop_front();
    }
    history.push_back(sample.clone());

    tracing::info!("[{}] value = {}", sample.ts, sample.value);

    (
        StatusCode::OK,
        Json(serde_json::json!({ "ok": true, "stored": history.len() })),
    )
}

async fn history(State(state): State<AppState>) -> Json<Vec<Sample>> {
    let history = state.history.lock().await;
    Json(history.iter().cloned().collect())
}

fn timestamp_now() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let secs = now % 60;
    let mins = (now / 60) % 60;
    let hours = (now / 3600) % 24;
    format!("{:02}:{:02}:{:02}", hours, mins, secs)
}
