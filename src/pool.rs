use std::collections::{BTreeMap, VecDeque};
use crate::models::{Player, MATCH_SIZE};

// ---------------------------------------------------------------------------
// PlayerPool
//
// Internally a BTreeMap<mmr, VecDeque<Player>>.
//
// BTreeMap keeps keys sorted by MMR, which means a range scan like
// "give me every player between MMR 1050 and 1350" is O(log n + k)
// rather than O(n). That's the entire reason we use BTreeMap over HashMap.
//
// VecDeque at each MMR bucket gives O(1) pop_front so we naturally
// serve the player who arrived earliest at that rating.
// ---------------------------------------------------------------------------

pub struct PlayerPool {
    inner: BTreeMap<u32, VecDeque<Player>>,
    total: usize,
}

impl PlayerPool {
    pub fn new() -> Self {
        PlayerPool {
            inner: BTreeMap::new(),
            total: 0,
        }
    }

    pub fn add(&mut self, player: Player) {
        self.inner
            .entry(player.mmr)
            .or_insert_with(VecDeque::new)
            .push_back(player);
        self.total += 1;
    }

    pub fn size(&self) -> usize {
        self.total
    }

    // Find the player who has been waiting longest across the whole pool.
    // Returns (mmr, window) so the caller can use it as a match anchor.
    // This is O(n) over all buckets in the worst case but n is bounded by
    // the queue size which stays small once matching is working.
    pub fn find_anchor(&self) -> Option<(u32, u32)> {
        self.inner
            .values()
            .flat_map(|bucket| bucket.iter())
            .min_by_key(|p| p.joined_at)
            .map(|p| (p.mmr, p.current_window()))
    }

    // Scan the MMR range [center - window, center + window] and collect
    // up to MATCH_SIZE player IDs. Returns IDs only — we hold no references
    // into the pool so the read lock can be dropped immediately.
    pub fn candidates_in_range(&self, center: u32, window: u32) -> Vec<String> {
        let lo = center.saturating_sub(window);
        let hi = center.saturating_add(window);

        let mut ids = Vec::with_capacity(MATCH_SIZE);

        for (_mmr, bucket) in self.inner.range(lo..=hi) {
            for player in bucket.iter() {
                ids.push(player.id.clone());
                if ids.len() == MATCH_SIZE {
                    return ids;
                }
            }
        }

        ids
    }

    // Atomic eviction — called under write lock.
    //
    // We pass in the IDs we found under read lock. Before removing anything
    // we verify every single one still exists. If even one is missing (taken
    // by another worker between read and write lock) we abort entirely and
    // return None. This is how we solve the TOCTOU race.
    pub fn remove_if_all_present(&mut self, ids: &[String]) -> Option<Vec<Player>> {
        // First pass — verify all IDs are still present
        for id in ids {
            if !self.id_exists(id) {
                return None;
            }
        }

        // Second pass — remove them all. Safe because we verified above
        // and we hold the write lock so nothing else can touch the pool.
        let mut players = Vec::with_capacity(ids.len());
        for id in ids {
            // unwrap is safe — we just verified existence
            players.push(self.remove_by_id(id).unwrap());
        }

        Some(players)
    }

    // Check whether a player ID exists anywhere in the pool.
    fn id_exists(&self, id: &str) -> bool {
        self.inner
            .values()
            .any(|bucket| bucket.iter().any(|p| p.id == id))
    }

    // Find and remove a player by ID. Returns None if not found.
    fn remove_by_id(&mut self, id: &str) -> Option<Player> {
        let mut target_mmr: Option<u32> = None;
        let mut target_idx: Option<usize> = None;

        'outer: for (mmr, bucket) in self.inner.iter() {
            for (idx, player) in bucket.iter().enumerate() {
                if player.id == id {
                    target_mmr = Some(*mmr);
                    target_idx = Some(idx);
                    break 'outer;
                }
            }
        }

        if let (Some(mmr), Some(idx)) = (target_mmr, target_idx) {
            let bucket = self.inner.get_mut(&mmr).unwrap();
            let player = bucket.remove(idx).unwrap();
            if bucket.is_empty() {
                self.inner.remove(&mmr);
            }
            self.total -= 1;
            return Some(player);
        }

        None
    }
}
