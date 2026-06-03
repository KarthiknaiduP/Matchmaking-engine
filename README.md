# Matchmaking Engine

A high-performance 5v5 matchmaking engine written in Rust. Holds players in memory, groups them into balanced teams of 5, and processes thousands of concurrent requests without dropping throughput.

---

## Running it

```bash
# Start the server (release build — ~5x faster than debug)
cargo run --release

# In another terminal, inject 5000 players
pip install aiohttp
python3 simulate.py

# Smaller run
python3 simulate.py --players 1000

# Custom concurrency
python3 simulate.py --players 10000 --concurrency 500
```

Endpoints:

| Method | Path       | Description                          |
|--------|------------|--------------------------------------|
| POST   | `/queue`   | Add a player to the matchmaking pool |
| GET    | `/metrics` | Live counters — queue size, match rate, avg MMR diff |
| GET    | `/matches` | Last 20 completed matches            |
| GET    | `/health`  | Liveness probe                       |

```bash
# Queue a player manually
curl -X POST http://localhost:3000/queue \
  -H "Content-Type: application/json" \
  -d '{"mmr": 1200, "region": "us-east"}'

# Check metrics
curl http://localhost:3000/metrics | python3 -m json.tool
```

---

## How I tackled each engineering challenge

### 1. Latency vs match quality

The core tension: matching fast means accepting mismatched players; matching well means some players wait forever.

The solution is time-based window relaxation. Every player enters the queue with a base MMR window of ±150. Every 30 seconds they wait, the window expands by ±50:

```
t = 0s   → ±150  (tight — only well-matched opponents)
t = 30s  → ±200
t = 60s  → ±250
t = 5min → ±650  (effectively open queue — someone will match)
```

Workers always scan starting from the longest-waiting player as the anchor. This means a fringe player (very high or very low MMR) won't wait indefinitely — their window just keeps growing until someone fits.

The initial window of 150 was chosen because at 1200 median MMR, a ±150 spread means the worst team composition has about a 300 MMR gap between its strongest and weakest player. That's tight enough to feel fair but wide enough that matches form within a few seconds under normal load.

### 2. Thread-safe state and atomic eviction

The pool is wrapped in `Arc<parking_lot::RwLock<PlayerPool>>`. I chose `parking_lot::RwLock` over `std::sync::RwLock` for two reasons: it uses adaptive spinning under contention (faster on high-core counts) and it doesn't poison on panic (a panicking writer won't permanently break the lock for every future reader).

The RwLock semantics matter: N workers can hold the read lock simultaneously while scanning for candidates. The write lock is only taken when evicting matched players. This means most of the time when workers are scanning but not yet ready to commit a match so they don't block each other at all.

The eviction itself is atomic by design:

1. Acquire **read lock** → find 10 candidate IDs → drop lock
2. Acquire **write lock** → re-verify all 10 still exist → remove them → drop lock

The re-verify step in stage 2 is critical. Without it you get a TOCTOU (time-of-check-to-time-of-use) race: two workers could each see the same player under their respective read locks, both decide to include them, and both try to put them in a match. Re-verifying under the write lock makes eviction truly atomic, if even one player was grabbed by another worker, the whole attempt aborts and retries.

### 3. Time-based constraint relaxation

Implemented directly on the `Player` struct:

```rust
pub fn current_window(&self) -> u32 {
    let ticks = self.wait_secs() / 30;
    150 + (ticks as u32 * 50)
}
```

The worker always uses the real-time window at scan time, not the window from when the player joined. This means a player who joined 90 seconds ago is automatically scanned with ±250 even if the worker hasn't seen them before.

The anchor selection picks the longest-waiting player as the starting point for each scan ensuring fairness. A worker won't keep re-matching fresh players while a veteran waits. The queue is effectively priority-ordered by wait time.

### 4. Team balance

Given 10 players, split into two teams of 5 that are as close in total MMR as possible.

Algorithm: sort by MMR descending, alternate assignment.

```
Sorted:  [2000, 1900, 1800, 1700, 1600, 1500, 1400, 1300, 1200, 1100]
Team A:   2000,       1800,       1600,       1400,       1200  = 9000 avg 1800
Team B:         1900,       1700,       1500,       1300, 1100  = 8500 avg 1700
Diff:     100 MMR
```

I considered the DP approach (exact optimal partition) but rejected it for this case. The partition problem on 10 elements with MMR values is bounded by the greedy sort-and-alternate gets within a few percent of optimal and runs in O(n log n) vs O(n × sum) for DP. More importantly, the greedy result is easy to reason about: the two teams are structurally mirror images of each other, which players find intuitive.

In practice, with the ±150–±250 MMR windows we're working with, the worst-case team imbalance is well under 200 MMR which is less than the spread within a team itself.

### 5. Low-latency health metrics

Every counter in the `Metrics` struct is an `AtomicU64`. Atomic operations are implemented as a single CPU instruction (e.g. `LOCK XADD` on x86) — there is no mutex, no kernel call, no scheduler involvement. A worker incrementing `total_matched` cannot block an HTTP handler reading `matches_made`. The `/metrics` endpoint only acquires one read lock (to read `queue_size`) and reads everything else lock-free.

For the running average MMR diff I track a `mmr_diff_sum` and `diff_count` pair. Dividing on read gives the current average without storing per-match history. Memory usage for metrics is O(1) regardless of how many matches have been made.

---

## Time and space complexity

| Operation              | Time           | Space   | Why                                         |
|------------------------|----------------|---------|---------------------------------------------|
| Queue player (insert)  | O(log n)       | O(1)    | BTreeMap insert by MMR key                  |
| Find anchor            | O(n)           | O(1)    | Full scan for min joined_at                 |
| Candidate range scan   | O(log n + k)   | O(k)    | BTreeMap .range(), k = players in window    |
| ID existence check     | O(n)           | O(1)    | Linear search by ID across buckets          |
| Atomic eviction        | O(n × m)       | O(m)    | m evictions, each O(n) removal              |
| Team balance           | O(m log m)     | O(m)    | Sort m=10 players                           |
| Metrics read           | O(1)           | O(1)    | AtomicU64 loads, one pool read lock         |
| Total pool memory      | O(n)           | —       | n = players currently in queue              |

Where n = total players in pool, k = players in the scanned MMR window, m = match size (10).

The O(n) ID lookup in `remove_by_id` is the weakest point. At 10,000 queued players a single eviction scan is still microseconds on modern hardware, but it would become the bottleneck before anything else. The fix is an additional `HashMap<player_id → mmr>` index, making lookups O(1) at the cost of keeping two structures in sync.

---

## Scaling challenges

This implementation is a single-process, single-machine service. It handles tens of thousands of concurrent players on one machine. Here's where it breaks and how you'd fix it:

**RwLock contention at very high player counts**

A single `RwLock<PlayerPool>` is a serialization point for writes. At 100k+ players/sec the write lock becomes a bottleneck. The fix is MMR sharding: split the pool into, say, 8 shards each covering a range of MMR values (0–400, 400–800, ...). Workers claim a shard, scan it, and only compete with other workers in the same MMR tier. Cross-shard matches (players at the boundary) are handled by the relaxation mechanism eventually widening their window.

**Multi-machine deployment**

A second machine can't see this machine's in-memory pool. To scale horizontally you'd move the pool to a shared store (Redis with sorted sets — `ZRANGEBYSCORE` maps cleanly to the BTreeMap range scan). Workers become stateless consumers that pop player groups from Redis, balance teams, and emit match events to a message queue (Kafka or Redis Streams). The HTTP tier also becomes stateless and any machine can accept a `/queue` POST and write to Redis.

**The O(n) ID lookup**

As mentioned above, a `HashMap<String, u32>` index mapping `player_id → mmr` would make `remove_by_id` O(1). The BTreeMap keeps the range-scan property; the HashMap adds fast point lookup. Both are updated atomically inside the write lock.

**Worker count**

The current `NUM_WORKERS = 4` constant is a decent default for a 4-core machine. On a 32-core production box you'd want more workers but benchmark first — adding workers past the point where RwLock write contention dominates doesn't help and adds context-switch overhead.


-----

## Result

By Running the simulation.py

<img width="440" height="858" alt="Screenshot 2026-06-04 at 12 12 17 AM" src="https://github.com/user-attachments/assets/dd9d5ef3-bcdb-456f-94b1-d3af04dec513" />
