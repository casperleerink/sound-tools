"""Runs both spike apps in real windows, scenario by scenario, alternating apps, and reports CPU
and memory of each process. CPU: CPU time used between 2 s after launch and 1 s before it quits,
divided by that wall time. Usage: python3 measure.py [--runs 3] [--seconds 12] [--tracks 8 --clips 32]"""

import argparse
import json
import statistics
import subprocess
import time

APPS = {
    "gpui": "/tmp/spike-target-gpui/release/gpui-daw",
    "rune": "/tmp/spike-target-rune/release/rune-daw",
}
SCENARIOS = ["idle", "play", "scroll", "zoom", "edit"]
WARMUP = 2.0


def cputime(pid):
    out = subprocess.run(["ps", "-o", "cputime=,rss=", "-p", str(pid)], capture_output=True, text=True).stdout.split()
    if len(out) < 2:
        return None, None
    parts = out[0].replace("-", ":").split(":")
    seconds = 0.0
    for part in parts:
        seconds = seconds * 60 + float(part)
    return seconds, int(out[1]) / 1024


def run(app, scenario, seconds, tracks, clips):
    command = [APPS[app], "--scenario", scenario, "--seconds", str(seconds), "--tracks", str(tracks), "--clips", str(clips)]
    process = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
    started = time.monotonic()
    time.sleep(WARMUP)
    cpu0, _ = cputime(process.pid)
    t0 = time.monotonic()
    rss = []
    while time.monotonic() - started < seconds - 1.0:
        _, mb = cputime(process.pid)
        if mb:
            rss.append(mb)
        time.sleep(0.25)
    cpu1, _ = cputime(process.pid)
    t1 = time.monotonic()
    out, _ = process.communicate(timeout=30)
    report = json.loads(out.strip().splitlines()[-1]) if out.strip() else {}
    return {
        "cpu %": 100 * (cpu1 - cpu0) / (t1 - t0) if cpu0 is not None and cpu1 is not None else None,
        "rss MB": statistics.median(rss) if rss else None,
        "frames/s": report.get("frames", 0) / seconds,
        "view renders/s": report.get("view_renders", 0) / seconds,
    }


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--runs", type=int, default=3)
    parser.add_argument("--seconds", type=float, default=12)
    parser.add_argument("--tracks", type=int, default=8)
    parser.add_argument("--clips", type=int, default=32)
    parser.add_argument("--scenarios", default=",".join(SCENARIOS))
    parser.add_argument("--out")
    a = parser.parse_args()
    results = {}
    for r in range(a.runs):
        for scenario in a.scenarios.split(","):
            for app in (APPS if r % 2 == 0 else reversed(list(APPS))):
                result = run(app, scenario, a.seconds, a.tracks, a.clips)
                print(f"run {r + 1} {scenario:6} {app}: {result}", flush=True)
                results.setdefault((scenario, app), []).append(result)
    print(f"\n{a.tracks} tracks, {a.clips} clips. Median of {a.runs} runs, {a.seconds} s each.\n")
    print(f"{'scenario':8} {'app':5} {'cpu %':>7} {'rss MB':>7} {'frames/s':>9} {'renders/s':>10}")
    table = {}
    for (scenario, app), runs in results.items():
        med = {k: statistics.median([x[k] for x in runs if x[k] is not None] or [0]) for k in runs[0]}
        table[f"{scenario}/{app}"] = med
        print(f"{scenario:8} {app:5} {med['cpu %']:7.1f} {med['rss MB']:7.0f} {med['frames/s']:9.1f} {med['view renders/s']:10.1f}")
    if a.out:
        with open(a.out, "w") as f:
            json.dump({"tracks": a.tracks, "clips": a.clips, "runs": results and {f"{s}/{p}": v for (s, p), v in results.items()}, "median": table}, f, indent=1)


if __name__ == "__main__":
    main()
