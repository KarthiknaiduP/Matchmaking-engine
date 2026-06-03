#!/usr/bin/env python3


import asyncio
import aiohttp
import argparse
import random
import time
import statistics
import json


BASE_URL = "http://localhost:3000"


def random_mmr() -> int:
    return max(100, min(3000, int(random.gauss(1200, 300)))) #for edge cases


def random_region() -> str:
    regions = ["us-east", "us-west", "eu-west", "ap-south", "ap-east"]
    return random.choice(regions)


async def add_player(
    session: aiohttp.ClientSession,
    semaphore: asyncio.Semaphore,
    results: list,
) -> None:
    async with semaphore:
        payload = {"mmr": random_mmr(), "region": random_region()}
        start = time.monotonic()
        try:
            async with session.post(
                f"{BASE_URL}/queue",
                json=payload,
                timeout=aiohttp.ClientTimeout(total=10),
            ) as resp:
                elapsed_ms = (time.monotonic() - start) * 1000
                body = await resp.json()
                results.append({
                    "status": resp.status,
                    "ms": elapsed_ms,
                    "mmr": payload["mmr"],
                })
        except asyncio.TimeoutError:
            results.append({"status": 0, "ms": 10000, "error": "timeout"})
        except Exception as e:
            results.append({"status": 0, "ms": 0, "error": str(e)})


async def poll_metrics(duration_secs: int, interval_secs: float = 2.0) -> None:
    print(f"\n{'─'*60}")
    print(f"  Polling /metrics every {interval_secs}s for {duration_secs}s")
    print(f"{'─'*60}")
    print(f"  {'Time':>6}  {'Queue':>8}  {'Matches':>8}  {'Matched':>10}  {'Avg Diff':>10}")
    print(f"{'─'*60}")

    start = time.monotonic()
    tick = 0

    async with aiohttp.ClientSession() as session:
        while (time.monotonic() - start) < duration_secs:
            try:
                async with session.get(
                    f"{BASE_URL}/metrics",
                    timeout=aiohttp.ClientTimeout(total=3),
                ) as resp:
                    data = await resp.json()
                    elapsed = time.monotonic() - start
                    print(
                        f"  {elapsed:>5.1f}s"
                        f"  {data['queue_size']:>8}"
                        f"  {data['total_matches_made']:>8}"
                        f"  {data['total_players_matched']:>10}"
                        f"  {data['avg_match_mmr_diff']:>9.1f}"
                    )
            except Exception as e:
                print(f"  metrics error: {e}")

            await asyncio.sleep(interval_secs)
            tick += 1


async def main(num_players: int, concurrency: int, poll_secs: int) -> None:
    print(f"\n{'═'*60}")
    print(f"  Matchmaking Engine — Load Simulation")
    print(f"{'═'*60}")
    print(f"  Players   : {num_players:,}")
    print(f"  Concurrency: {concurrency}")
    print(f"  Target    : {BASE_URL}")
    print(f"{'═'*60}\n")

    # Quick health check before flooding
    async with aiohttp.ClientSession() as session:
        try:
            async with session.get(f"{BASE_URL}/health", timeout=aiohttp.ClientTimeout(total=3)) as r:
                if r.status != 200:
                    print(f"  ERROR: /health returned {r.status} — is the server running?")
                    return
                print(f"  Server is up ✓\n")
        except Exception as e:
            print(f"  ERROR: cannot reach {BASE_URL} — {e}")
            print(f"  Start the server first:  cargo run --release")
            return

    results = []
    semaphore = asyncio.Semaphore(concurrency)

    inject_start = time.monotonic()

    async with aiohttp.ClientSession() as session:
        tasks = [
            add_player(session, semaphore, results)
            for _ in range(num_players)
        ]

        # Run all requests; print progress every 10%
        chunk = max(1, num_players // 10)
        done = 0
        for coro in asyncio.as_completed(tasks):
            await coro
            done += 1
            if done % chunk == 0 or done == num_players:
                pct = done / num_players * 100
                ok = sum(1 for r in results if r["status"] == 200)
                print(f"  {pct:5.0f}%  injected {done:>6,} / {num_players:,}  ok={ok}")

    inject_elapsed = time.monotonic() - inject_start

    # ── Summary ---
    ok = [r for r in results if r["status"] == 200]
    failed = [r for r in results if r["status"] != 200]
    latencies = sorted(r["ms"] for r in ok)

    print(f"\n{'═'*60}")
    print(f"  Injection complete in {inject_elapsed:.2f}s")
    print(f"{'─'*60}")
    print(f"  Successful  : {len(ok):,} / {num_players:,}")
    print(f"  Failed      : {len(failed):,}")

    if latencies:
        p50  = latencies[len(latencies) // 2]
        p95  = latencies[int(len(latencies) * 0.95)]
        p99  = latencies[int(len(latencies) * 0.99)]
        mean = statistics.mean(latencies)
        print(f"{'─'*60}")
        print(f"  Latency (queue endpoint)")
        print(f"    mean : {mean:.1f} ms")
        print(f"    p50  : {p50:.1f} ms")
        print(f"    p95  : {p95:.1f} ms")
        print(f"    p99  : {p99:.1f} ms")
        print(f"    max  : {latencies[-1]:.1f} ms")

    if ok:
        throughput = len(ok) / inject_elapsed
        print(f"{'─'*60}")
        print(f"  Throughput : {throughput:.0f} req/s")

    # ── Poll metrics while workers clear the queue ──
    await poll_metrics(duration_secs=poll_secs)

    print(f"\n{'═'*60}")
    print(f"  Done.")
    print(f"{'═'*60}\n")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Matchmaking engine simulation")
    parser.add_argument("--players",     type=int, default=5000,  help="total players to inject")
    parser.add_argument("--concurrency", type=int, default=200,   help="max concurrent requests")
    parser.add_argument("--poll",        type=int, default=30,    help="seconds to poll metrics after injection")
    args = parser.parse_args()

    asyncio.run(main(args.players, args.concurrency, args.poll))
