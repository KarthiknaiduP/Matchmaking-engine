mod balance;
mod handlers;
mod metrics;
mod models;
mod pool;
mod state;
mod worker;

use axum::{routing::{get, post}, Router};
use std::thread;
use tracing::info;
use tracing_subscriber::EnvFilter;

use handlers::{get_matches, get_metrics, health, queue_player};
use state::AppState;

// Number of concurrent matching workers is number of logical CPU cores because
// Each worker is a real OS thread so they run in parallel on multi-core
const NUM_WORKERS: usize = 4;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    info!("Starting matchmaking engine");

    let state = AppState::new();

    // Spawn matching workers on dedicated OS threads.
    // We use std::thread rather than tokio::spawn because the worker loop
    // is CPU-bound (scanning the pool). Blocking a tokio thread would starve
    // async I/O. OS threads give us true parallelism on multi-core hardware.
    
    for worker_id in 0..NUM_WORKERS {
        let worker_state = state.clone();
        thread::Builder::new()
            .name(format!("worker-{}", worker_id))
            .spawn(move || worker::run_worker(worker_state, worker_id))
            .expect("failed to spawn worker thread");
    }

    info!("Spawned {} matching workers", NUM_WORKERS);

    let app = Router::new()
        .route("/health",  get(health))
        .route("/queue",   post(queue_player))
        .route("/metrics", get(get_metrics))
        .route("/matches", get(get_matches))
        .with_state(state);

    let addr = "0.0.0.0:3000";
    info!("Listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("failed to bind port 3000");

    axum::serve(listener, app)
        .await
        .expect("server error");
}
