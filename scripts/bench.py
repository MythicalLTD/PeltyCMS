#!/usr/bin/env python3
"""Tiny HTTP load tool (keep-alive, N threads, fixed duration).
Usage: bench.py http://127.0.0.1:18090 /path [/path ...] [-c 16] [-d 10] [-H 'Cookie: ...']
Prints req/s and p50/p95/p99 latency per path."""
import argparse, http.client, threading, time, urllib.parse

ap = argparse.ArgumentParser()
ap.add_argument("base"); ap.add_argument("paths", nargs="+")
ap.add_argument("-c", type=int, default=16); ap.add_argument("-d", type=float, default=10)
ap.add_argument("-H", action="append", default=[])
a = ap.parse_args()
u = urllib.parse.urlparse(a.base)
hdrs = dict(h.split(": ", 1) for h in a.H)

def run(path):
    lat, errs, stop = [], [0], time.time() + a.d
    lock = threading.Lock()
    def worker():
        c = http.client.HTTPConnection(u.hostname, u.port, timeout=10)
        mine = []
        while time.time() < stop:
            t = time.perf_counter()
            try:
                c.request("GET", path, headers=hdrs); r = c.getresponse(); r.read()
                if r.status >= 400: errs[0] += 1
            except Exception:
                errs[0] += 1; c = http.client.HTTPConnection(u.hostname, u.port, timeout=10); continue
            mine.append(time.perf_counter() - t)
        with lock: lat.extend(mine)
    ts = [threading.Thread(target=worker) for _ in range(a.c)]
    [t.start() for t in ts]; [t.join() for t in ts]
    lat.sort(); n = len(lat)
    q = lambda p: lat[min(n - 1, int(n * p))] * 1000 if n else 0
    print(f"{path:40s} req/s={n / a.d:8.1f} p50={q(.5):7.2f}ms p95={q(.95):7.2f}ms p99={q(.99):7.2f}ms errors={errs[0]}")

for p in a.paths: run(p)
