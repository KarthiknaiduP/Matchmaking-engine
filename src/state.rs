use std::sync::Arc;
use parking_lot::RwLock;
use crate::metrics::Metrics;
use crate::models::MatchRecord;
use crate::pool::PlayerPool;

// ---------------------------------------------------------------------------
// AppState
//
// Single struct that wraps every piece of shared mutable state.
// It's Clone because axum requires State<T>: Clone — the Arc clones are
// cheap (just incrementing a reference count).
//
// Why parking_lot::RwLock instead of std::sync::RwLock?
//   - parking_lot is faster under high contention (uses adaptive spinning)
//   - It doesn't poison on panic — std RwLock turns a panicking writer into
//     a permanently broken lock that poisons every future reader too
//   - The API is slightly nicer (no unwrap needed on lock())
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct AppState {
    pub pool: Arc<RwLock<PlayerPool>>,
    pub metrics: Arc<Metrics>,
    pub recent_matches: Arc<RwLock<Vec<MatchRecord>>>,
}

impl AppState {
    pub fn new() -> Self {
        AppState {
            pool: Arc::new(RwLock::new(PlayerPool::new())),
            metrics: Arc::new(Metrics::new()),
            recent_matches: Arc::new(RwLock::new(Vec::new())),
        }
    }
}
