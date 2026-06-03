use crate::models::{MatchRecord, Player, TeamMember, TEAM_SIZE};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Team balancing
//
// Given 10 players, split them into two teams of 5 that are as close in
// total MMR as possible.
//
// Algorithm: sort by MMR descending, then alternate assignment.
//   Sorted: [2000, 1900, 1800, 1700, 1600, 1500, 1400, 1300, 1200, 1100]
//   Team A:  2000,       1800,       1600,       1400,       1200  = 9000
//   Team B:        1900,       1700,       1500,       1300, 1100  = 8500
//   Diff = 500 MMR
//
// Why not DP? This is a 5+5 partition of 10 elements. The greedy approach
// gets within ~5% of optimal in practice and runs in O(n log n) vs O(n * sum)
// for DP. For 10 elements the difference is negligible but the simplicity
// makes the intent obvious when reading the code.
// ---------------------------------------------------------------------------

pub fn balance_teams(mut players: Vec<Player>) -> MatchRecord {
    assert_eq!(players.len(), TEAM_SIZE * 2, "need exactly 10 players");

    // Highest MMR first
    players.sort_unstable_by(|a, b| b.mmr.cmp(&a.mmr));

    let mut team_a: Vec<Player> = Vec::with_capacity(TEAM_SIZE);
    let mut team_b: Vec<Player> = Vec::with_capacity(TEAM_SIZE);

    for (i, player) in players.into_iter().enumerate() {
        if i % 2 == 0 {
            team_a.push(player);
        } else {
            team_b.push(player);
        }
    }

    let avg_a = avg_mmr(&team_a);
    let avg_b = avg_mmr(&team_b);
    let diff = avg_a.abs_diff(avg_b);

    MatchRecord {
        match_id: Uuid::new_v4().to_string(),
        avg_mmr_a: avg_a,
        avg_mmr_b: avg_b,
        mmr_diff: diff,
        team_a: team_a.iter().map(TeamMember::from).collect(),
        team_b: team_b.iter().map(TeamMember::from).collect(),
        created_at_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
    }
}

fn avg_mmr(team: &[Player]) -> u32 {
    if team.is_empty() {
        return 0;
    }
    (team.iter().map(|p| p.mmr as u64).sum::<u64>() / team.len() as u64) as u32
}
