"""Measures what the window costs per scenario on a project, and compares with a saved run. See README.md."""

import argparse
import json
import os
import subprocess
import sys
import time
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
ROOT = Path(os.environ.get("PERF_DIR", "/tmp/st-perf")) / "ui"
TEST = "tools::scenarios::measure::measure_the_window"
# The headline of each scenario: (label, path in its JSON).
HEADLINE = [
    ("update ms", ("update ms", "mean")),
    ("draw ms", ("draw ms", "mean")),
    ("allocations", ("allocations per update", "mean")),
    ("MB allocated", ("allocated bytes per update", "mean")),
    ("nodes drawn", ("nodes drawn per update",)),
]


def measure(project, out):
    env = dict(os.environ, UI_PERF_OUT=str(out))
    if project:
        env["UI_PERF_PROJECT"] = str(Path(project).resolve())
    command = ["cargo", "test", "--quiet", "-p", "runtime", "--test", "window", "--features", "perf",
               "--profile", "perf", "--", "--ignored", "--exact", TEST, "--nocapture", "--test-threads=1"]
    subprocess.run(command, cwd=REPO, env=env, check=True)
    return json.loads(out.read_text())


def flatten(value, path=()):
    """Every number in a scenario's JSON, by its path."""
    if isinstance(value, dict):
        for key, inner in value.items():
            yield from flatten(inner, (*path, key))
    elif isinstance(value, (int, float)) and not isinstance(value, bool):
        yield path, value


def headline(scenario):
    values = dict(flatten(scenario))
    cells = {label: values.get(path) for label, path in HEADLINE}
    if cells["MB allocated"] is not None:
        cells["MB allocated"] /= 2**20
    for part, by_instance in scenario.get("renders", {}).items():
        cells[f"{part} renders"] = sum(by_instance.values())
    return cells


def show(report, against):
    print(f"\n{report['project']}. Per update, mean. Renders: all instances of a part, over all updates.\n")
    for name, scenario in report["scenarios"].items():
        if "updates" not in scenario:
            print(f"{name}: {scenario}")
            continue
        before = (against or {}).get("scenarios", {}).get(name, {})
        now, then = headline(scenario), headline(before) if "updates" in before else {}
        cells = []
        for label, value in now.items():
            if value is None:
                continue
            old = then.get(label)
            change = f" ({(value - old) / old * 100:+.0f}%)" if old else (" (new)" if before and old is None else "")
            cells.append(f"{label} {value:.2f}{change}" if isinstance(value, float) else f"{label} {value}{change}")
        print(f"{name}, {scenario['updates']} updates: " + ", ".join(cells))
    if against:
        print("\nEvery other number that moved by 20% or more:\n")
        for name, scenario in report["scenarios"].items():
            old = dict(flatten(against.get("scenarios", {}).get(name, {})))
            for path, value in flatten(scenario):
                if path[-1] in ("count", "updates") or path in [p for _, p in HEADLINE]:
                    continue
                base = old.get(path)
                if base and abs(value - base) / base >= 0.2:
                    print(f"{name} / {' / '.join(path)}: {base:.4g} -> {value:.4g}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("project", nargs="?", help="a project folder; by default the song of the DAW template")
    parser.add_argument("--out", type=Path, help="where to write the JSON")
    parser.add_argument("--against", type=Path, help="a saved run to compare with")
    arguments = parser.parse_args()
    ROOT.mkdir(parents=True, exist_ok=True)
    out = arguments.out or ROOT / f"{Path(arguments.project or 'daw').name}-{time.strftime('%Y%m%d-%H%M%S')}.json"
    against = json.loads(arguments.against.read_text()) if arguments.against else None
    report = measure(arguments.project, out.resolve())
    show(report, against)
    print(f"\nwrote {out}", file=sys.stderr)


if __name__ == "__main__":
    main()
