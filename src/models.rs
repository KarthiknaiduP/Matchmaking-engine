use std::time::Instant;
use serde::{Deserialize, Serialize};

// Base MMR window when a player first joins.
// Expands by WINDOW_EXPANSION_PER_TICK every RELAXATION_TICK_SECS seconds of waiting.
pub const BASE_WINDOW: u32 = 150;
pub const RELAXATION_TICK_SECS: u64 = 30;
pub const WINDOW_EXPANSION_PER_TICK: u32 = 50;
pub const MATCH_SIZE: usize = 10;
pub const TEAM_SIZE: usize = 5;

// ---------------------------------------------------------------------------
// Player
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Player {
    pub id: String,
    pub mmr: u32,
    pub region: String,
    pub joined_at: Instant,
}

impl Player {
    pub fn new(id: String, mmr: u32, region: String) -> Self {
        Player {
            id,
            mmr,
            region,
            joined_at: Instant::now(),
        }
    }

    pub fn wait_secs(&self) -> u64 {
        self.joined_at.elapsed().as_secs()
    }

    // MMR window expands the longer a player waits.
    // t=0s  → ±150
    // t=30s → ±200
    // t=60s → ±250
    // t=5m  → ±650  (effectively open queue)
    pub fn current_window(&self) -> u32 {
        let ticks = self.wait_secs() / RELAXATION_TICK_SECS;
        BASE_WINDOW + (ticks as u32 * WINDOW_EXPANSION_PER_TICK)
    }
}

// ---------------------------------------------------------------------------
// HTTP request/response shapes
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct QueueRequest {
    pub mmr: u32,
    pub region: String,
}

#[derive(Debug, Serialize)]
pub struct QueueResponse {
    pub player_id: String,
    pub queue_position: usize,
    pub estimated_window: u32,
}

// ---------------------------------------------------------------------------
// Match result
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct MatchRecord {
    pub match_id: String,
    pub team_a: Vec<TeamMember>,
    pub team_b: Vec<TeamMember>,
    pub avg_mmr_a: u32,
    pub avg_mmr_b: u32,
    pub mmr_diff: u32,
    pub created_at_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct TeamMember {
    pub player_id: String,
    pub mmr: u32,
}

impl From<&Player> for TeamMember {
    fn from(p: &Player) -> Self {
        TeamMember {
            player_id: p.id.clone(),
            mmr: p.mmr,
        }
    }
}

// ---------------------------------------------------------------------------
// Metrics snapshot for the /metrics endpoint
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct MetricsSnapshot {
    pub queue_size: usize,
    pub total_players_queued: u64,
    pub total_players_matched: u64,
    pub total_matches_made: u64,
    pub avg_match_mmr_diff: f64,
}
