use std::time::Duration;
use tracing::{debug, info};
use crate::balance::balance_teams;
use crate::models::MATCH_SIZE;
use crate::state::AppState;

// ---------------------------------------------------------------------------
// Matching worker
//
// Each worker runs in an infinite loop on its own OS thread (not a tokio
// task — we want true parallelism here, not cooperative scheduling).
//
// The loop:
//   1. Acquire READ lock → find longest-waiting player as anchor
//   2. Under READ lock → collect up to 10 candidate IDs in their MMR window
//   3. Drop READ lock
//   4. If fewer than 10 → sleep and retry
//   5. Acquire WRITE lock → atomically evict all 10 (re-verify they exist)
//   6. Drop WRITE lock
//   7. Balance teams, record match, update metrics
//   8. Sleep 10ms to yield to other workers and the HTTP thread
//
// The critical design choice is steps 1-3 vs 5-6: we hold the read lock
// only long enough to sample IDs, then drop it before acquiring the write
// lock. This means other workers + HTTP handlers can read the pool
// concurrently. The write lock is held for the minimum possible time.
// ---------------------------------------------------------------------------

const WORKER_SLEEP_MS: u64 = 10;
const RECENT_MATCHES_CAP: usize = 1000;

pub fn run_worker(state: AppState, worker_id: usize) {
    info!("Worker {} started", worker_id);

    loop {
        // ---- Phase 1: sample under read lock --------------------------------
        let maybe_group = {
            let pool = state.pool.read();

            let anchor = match pool.find_anchor() {
                Some(a) => a,
                None => {
                    drop(pool);
                    std::thread::sleep(Duration::from_millis(WORKER_SLEEP_MS));
                    continue;
                }
            };

            let (anchor_mmr, window) = anchor;
            let ids = pool.candidates_in_range(anchor_mmr, window);

            if ids.len() < MATCH_SIZE {
                debug!(
                    "Worker {}: only {}/{} candidates at MMR {} ±{}",
                    worker_id,
                    ids.len(),
                    MATCH_SIZE,
                    anchor_mmr,
                    window
                );
                None
            } else {
                Some(ids)
            }
            // read lock drops here
        };

        let ids = match maybe_group {
            Some(ids) => ids,
            None => {
                std::thread::sleep(Duration::from_millis(WORKER_SLEEP_MS));
                continue;
            }
        };

        // ---- Phase 2: atomic eviction under write lock ----------------------
        let players = {
            let mut pool = state.pool.write();
            pool.remove_if_all_present(&ids)
            // write lock drops here
        };

        let players = match players {
            Some(p) => p,
            None => {
                // Another worker grabbed some of these players between our
            // read and write lock. That's fine — just retry.
                debug!("Worker {}: eviction conflict, retrying", worker_id);
                continue;
            }
        };

        // ---- Phase 3: balance + record (no lock held) -----------------------
        let record = balance_teams(players);

        info!(
            "Worker {} → match {} | A avg:{} B avg:{} diff:{}",
            worker_id,
            &record.match_id[..8],
            record.avg_mmr_a,
            record.avg_mmr_b,
            record.mmr_diff
        );

        state.metrics.match_made(record.mmr_diff);

        {
            let mut matches = state.recent_matches.write();
            matches.push(record);
            // Keep memory bounded — drop old matches beyond the cap
            if matches.len() > RECENT_MATCHES_CAP {
                let overflow = matches.len() - RECENT_MATCHES_CAP;
                matches.drain(0..overflow);
            }
        }

        // No sleep after a successful match — immediately try to form another
    }
}
