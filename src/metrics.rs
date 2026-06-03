use std::sync::atomic::{AtomicU64, Ordering};

// ---------------------------------------------------------------------------
// Metrics
//
// Every counter here is an AtomicU64. The key property: atomic operations
// never acquire a lock. A worker thread incrementing `total_matched` cannot
// block an HTTP thread reading `queue_size`. This means /metrics responses
// have zero contention with the matchmaking loop — monitoring is genuinely
// free at runtime.
//
// Ordering::Relaxed is fine for counters that don't guard memory accesses —
// we only care that increments are atomic, not that they're sequentially
// consistent with other operations.
//
// mmr_diff_sum + diff_count together give a running average without
// storing per-match history.
// ---------------------------------------------------------------------------

pub struct Metrics {
    pub total_queued: AtomicU64,
    pub total_matched: AtomicU64,
    pub matches_made: AtomicU64,
    mmr_diff_sum: AtomicU64,
    diff_count: AtomicU64,
}

impl Metrics {
    pub fn new() -> Self {
        Metrics {
            total_queued: AtomicU64::new(0),
            total_matched: AtomicU64::new(0),
            matches_made: AtomicU64::new(0),
            mmr_diff_sum: AtomicU64::new(0),
            diff_count: AtomicU64::new(0),
        }
    }

    pub fn player_joined(&self) {
        self.total_queued.fetch_add(1, Ordering::Relaxed);
    }

    pub fn match_made(&self, mmr_diff: u32) {
        self.matches_made.fetch_add(1, Ordering::Relaxed);
        self.total_matched.fetch_add(10, Ordering::Relaxed);
        self.mmr_diff_sum.fetch_add(mmr_diff as u64, Ordering::Relaxed);
        self.diff_count.fetch_add(1, Ordering::Relaxed);
    }

    pub fn avg_mmr_diff(&self) -> f64 {
        let count = self.diff_count.load(Ordering::Relaxed);
        if count == 0 {
            return 0.0;
        }
        self.mmr_diff_sum.load(Ordering::Relaxed) as f64 / count as f64
    }
}
