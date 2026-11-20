"""What a frame of the window costs per scenario, headless, and the process's CPU and memory. See README.md."""

import argparse
import json
import os
import platform
import re
import shutil
import statistics
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
ROOT = Path(os.environ.get("PERF_DIR", "/tmp/st-perf")) / "window"
BASELINE = HERE / "window.json"
ENV = dict(os.environ, HOME=str(ROOT / "home"), SOUND_TOOLS_NO_UPDATES="1",
           PATH=os.pathsep.join([str(Path.home() / ".bun/bin"), os.environ.get("PATH", "")]))
# ru_maxrss is bytes on macOS and kilobytes on Linux.
MAXRSS_BYTES = 1 if platform.system() == "Darwin" else 1024
SECONDS = 3.0
# Where the song draws, in points, in the default window of 1470 x 920: the timeline starts
# right of the track heads, a row is 64 high, and a bar is 48 wide at the zoom a song opens at.
TIMELINE, FIRST_ROW, BAR_POINTS, TICKS_PER_BAR = 184, 124, 48, 1920
# The big song: 64 tracks, 500 clips of 2 bars, 8 notes each.
TRACKS, CLIPS, CLIP_BARS = 64, 500, 2
COLOURS = ["blue", "green", "purple", "orange", "pink", "teal", "yellow", "red"]


def run(program, *arguments, check=True):
    """Runs `sound-tools`. Returns its text and (cpu seconds, peak MB) of the process itself."""
    process = subprocess.Popen([program, *map(str, arguments)], env=ENV, stdout=subprocess.PIPE,
                               stderr=subprocess.STDOUT, text=True)
    text = process.stdout.read()
    _, status, usage = os.wait4(process.pid, 0)
    if check and status != 0:
        raise SystemExit(f"sound-tools {' '.join(map(str, arguments))} failed:\n{text}")
    return text, (usage.ru_utime + usage.ru_stime, usage.ru_maxrss * MAXRSS_BYTES / 2**20)


def program_of(path):
    if path:
        return Path(path).resolve()
    subprocess.run(["cargo", "build", "--release", "-p", "sound-window", "--locked"], cwd=REPO, check=True)
    metadata = subprocess.run(["cargo", "metadata", "--no-deps", "--format-version", "1"], cwd=REPO, check=True,
                              capture_output=True, text=True).stdout
    return Path(json.loads(metadata)["target_directory"]) / "release" / "sound-tools"


def new_project(program, folder):
    """A new project of the DAW template, as the app writes it into a missing folder."""
    shutil.rmtree(folder, ignore_errors=True)
    steps = ROOT / "no-steps.json"
    steps.write_text("[]")
    run(program, folder, "--act", steps, "--tree", ROOT / "tree.json")


def big_song(program, folder):
    """The DAW template with TRACKS tracks of the first track's devices and CLIPS clips."""
    new_project(program, folder)
    song = folder / "state/song"
    first = song / "t00"
    for track in sorted(song.glob("t*")):
        if track != first:
            shutil.rmtree(track)
    for row in range(TRACKS):
        track = song / f"t{row:02}"
        if row:
            shutil.copytree(first, track)
        for clip in track.glob("*.json"):
            if json.loads(clip.read_text())["tool"] == "timeline":
                clip.unlink()
        (track / "instance.json").write_text(json.dumps(
            {"tool": "track", "state": {"title": f"Track {row + 1}", "colour": COLOURS[row % len(COLOURS)]}}))
    length = CLIP_BARS * TICKS_PER_BAR
    for index in range(CLIPS):
        notes = [{"start": k * length // 8, "length": length // 8, "pitch": 48 + (index + k * 5) % 24, "velocity": 100}
                 for k in range(8)]
        record = {"tool": "timeline", "state": {"start": index // TRACKS * length, "length": length, "notes": notes}}
        (song / f"t{index % TRACKS:02}" / f"c{index:03}.json").write_text(json.dumps(record))


def scenarios(first_clip_bars):
    """Each scenario's script: events, the last at SECONDS."""
    clip = {"x": TIMELINE + first_clip_bars * BAR_POINTS / 2, "y": FIRST_ROW}
    steps = [0.2 + k / 60 for k in range(120)]
    end = [{"t": SECONDS, "type": "mark", "name": "end"}]
    return {
        "idle": end,
        "play": [{"t": 0.2, "type": "tap", "key": "play"}, *end],
        "scroll": [{"t": t, "type": "wheel", "key": "song", "dx": -6, "dy": 0} for t in steps] + end,
        "zoom": [{"t": t, "type": "wheel", "key": "song", "dx": 0, "dy": -2, "cmd": True} for t in steps] + end,
        "drag a clip": [{"t": 0.2, "type": "drag", **clip, "dx": 240, "dy": 64, "dur": 2.0}, *end],
        "note editor": [{"t": 0.2, "type": "double-click", **clip}, *end],
    }


FRAMES = re.compile(r"rune: (\d+) frames at .* frame cpu mean ([0-9.]+) ms p50 ([0-9.]+) p95 ([0-9.]+) max ([0-9.]+)")


def measure(program, project, name, events, runs):
    """Rune's frame times of a script, and the process's CPU and memory. Its picture: `<project>-<name>.png`."""
    script = ROOT / f"{project.name}-{name.replace(' ', '-')}.json"
    script.write_text(json.dumps(events))
    rows = []
    for _ in range(runs):
        text, (cpu, mb) = run(program, project, "--script", script, "--snap", script.with_suffix(".png"))
        found = FRAMES.search(text)
        if not found:
            raise SystemExit(f"{name}: no frame times in\n{text}")
        frames, mean, p50, p95, peak = found.groups()
        rows.append({"frames": int(frames), "mean ms": float(mean), "p50 ms": float(p50), "p95 ms": float(p95),
                     "max ms": float(peak), "cpu s": cpu, "MB": mb})
    return {key: statistics.median(row[key] for row in rows) for key in rows[0]}


def played(program, project, seconds, runs):
    """The app's own `--play`: the song from the start on the offline engine, one picture at the end."""
    picture = ROOT / f"{project.name}-play-{seconds}.png"
    rows = [run(program, project, "--snap", picture, "--play", seconds)[1] for _ in range(runs)]
    return {"cpu s": statistics.median(cpu for cpu, _ in rows), "MB": statistics.median(mb for _, mb in rows)}


def show(report, against):
    columns = ["frames", "p50 ms", "p95 ms", "max ms", "cpu s", "MB"]
    print(f"\nHeadless, {SECONDS:.0f} s of frames at 60 fps, median of {report['runs']} runs, load average "
          f"{report['load']:.0f}. Frame ms: Rune's wall time of a frame's work (poll, view, layout, encode). "
          "cpu s and MB: the sound-tools process, Bun not counted.\n")
    print("| project | scenario | " + " | ".join(columns) + " |")
    print("| --- | --- |" + " ---: |" * len(columns))
    for project, by_scenario in report["projects"].items():
        for name, now in by_scenario.items():
            then = (against or {}).get("projects", {}).get(project, {}).get(name, {})
            cells = []
            for column in columns:
                value, old = now.get(column), then.get(column)
                cell = "" if value is None else f"{value:.0f}" if column in ("frames", "MB") else f"{value:.2f}"
                if value is not None and old:
                    cell += f" ({(value - old) / old * 100:+.0f}%)"
                cells.append(cell)
            print(f"| {project} | {name} | " + " | ".join(cells) + " |")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--program", help="a sound-tools to measure; by default a release build of this checkout")
    parser.add_argument("--runs", type=int, default=3)
    parser.add_argument("--against", type=Path, default=BASELINE, help="a saved run to compare with")
    parser.add_argument("--save", action="store_true", help=f"keep this run as {BASELINE.relative_to(REPO)}")
    arguments = parser.parse_args()
    (ROOT / "home").mkdir(parents=True, exist_ok=True)
    program = program_of(arguments.program)
    projects = {"daw": (ROOT / "daw", new_project, 8), "big song": (ROOT / "big", big_song, CLIP_BARS)}
    report = {"program": run(program, "--version")[0].strip(), "machine": platform.platform(),
              "date": time.strftime("%Y-%m-%d"), "runs": arguments.runs, "projects": {}}
    report["load"] = os.getloadavg()[0]
    for project, (folder, make, first_clip_bars) in projects.items():
        print(f"measuring {project}", file=sys.stderr, flush=True)
        make(program, folder)
        results = {name: measure(program, folder, name, events, arguments.runs)
                   for name, events in scenarios(first_clip_bars).items()}
        results["--play 10"] = played(program, folder, 10, arguments.runs)
        report["projects"][project] = results
    against = json.loads(arguments.against.read_text()) if arguments.against.exists() else None
    show(report, against)
    out = BASELINE if arguments.save else ROOT / f"{time.strftime('%Y%m%d-%H%M%S')}.json"
    out.write_text(json.dumps(report, indent=1) + "\n")
    print(f"\nwrote {out}", file=sys.stderr)


if __name__ == "__main__":
    main()
