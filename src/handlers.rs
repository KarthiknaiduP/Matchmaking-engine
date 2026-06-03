use axum::{
    extract::State,
    http::StatusCode,
    Json,
};
use uuid::Uuid;

use crate::models::{
    MetricsSnapshot, QueueRequest, QueueResponse,
};
use crate::state::AppState;

// POST /queue
// Body: { "mmr": 1200, "region": "us-east" }
//
// Validates MMR range, creates a Player, inserts into pool.
// Returns the assigned player_id and current queue position.
pub async fn queue_player(
    State(state): State<AppState>,
    Json(req): Json<QueueRequest>,
) -> Result<Json<QueueResponse>, (StatusCode, String)> {
    // Basic validation — MMR must be a sane value
    if req.mmr == 0 || req.mmr > 10_000 {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("mmr must be between 1 and 10000, got {}", req.mmr),
        ));
    }

    if req.region.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "region cannot be empty".into()));
    }

    let player_id = Uuid::new_v4().to_string();
    let window = crate::models::BASE_WINDOW; // initial window before any wait

    let player = crate::models::Player::new(player_id.clone(), req.mmr, req.region);

    let queue_position = {
        let mut pool = state.pool.write();
        pool.add(player);
        pool.size()
    };

    state.metrics.player_joined();

    Ok(Json(QueueResponse {
        player_id,
        queue_position,
        estimated_window: window,
    }))
}

// GET /metrics
// Returns live counters. Reads are lock-free (AtomicU64 loads) except
// for queue_size which needs a read lock on the pool.
pub async fn get_metrics(State(state): State<AppState>) -> Json<MetricsSnapshot> {
    use std::sync::atomic::Ordering;

    let queue_size = state.pool.read().size();

    Json(MetricsSnapshot {
        queue_size,
        total_players_queued:  state.metrics.total_queued.load(Ordering::Relaxed),
        total_players_matched: state.metrics.total_matched.load(Ordering::Relaxed),
        total_matches_made:    state.metrics.matches_made.load(Ordering::Relaxed),
        avg_match_mmr_diff:    state.metrics.avg_mmr_diff(),
    })
}

// GET /matches
// Returns the last N completed matches from the in-memory history.
pub async fn get_matches(State(state): State<AppState>) -> Json<serde_json::Value> {
    let matches = state.recent_matches.read();
    let last_20: Vec<_> = matches.iter().rev().take(20).collect();
    Json(serde_json::json!({
        "total": matches.len(),
        "matches": last_20,
    }))
}

// GET /health
// Lightweight liveness probe — no lock acquisition, just a static string.
pub async fn health() -> &'static str {
    "OK"
}
