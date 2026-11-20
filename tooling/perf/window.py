"""What a frame of the window costs per scenario, headless, and the process's CPU and memory. See README.md."""

import argparse
import ctypes
import json
import os
import platform
import re
import select
import shutil
import statistics
import subprocess
import sys
import threading
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
ROOT = Path(os.environ.get("PERF_DIR", "/tmp/st-perf")) / "window"
BASELINE = HERE / "window.json"
ENV = dict(os.environ, HOME=str(ROOT / "home"), SOUND_TOOLS_NO_UPDATES="1",
           PATH=os.pathsep.join([str(Path.home() / ".bun/bin"), os.environ.get("PATH", "")]))
MAC = platform.system() == "Darwin"
# ru_maxrss is bytes on macOS and kilobytes on Linux.
MAXRSS_BYTES = 1 if MAC else 1024
LIBPROC = ctypes.CDLL("/usr/lib/libproc.dylib") if MAC else None
# `ri_phys_footprint` and `ri_lifetime_max_phys_footprint` of `rusage_info_v4` in
# <sys/resource.h>: after a uuid of 16 bytes, the 8th and the 29th number.
RUSAGE_INFO_V4, FOOTPRINT, LIFETIME_MAX_FOOTPRINT = 4, 16 + 7 * 8, 16 + 28 * 8
SECONDS = 3.0
# Where the song draws, in points, in the default window of 1470 x 920: the timeline starts
# right of the track heads, a row is 64 high, and a bar is 48 wide at the zoom a song opens at.
TIMELINE, FIRST_ROW, BAR_POINTS, TICKS_PER_BAR = 184, 124, 48, 1920
# The big song: 64 tracks, 500 clips of 2 bars, 8 notes each.
TRACKS, CLIPS, CLIP_BARS = 64, 500, 2
# The lanes: 2 tracks of 32 lanes on a wavetable's numbers, 200 points each, all shown.
LANE_TRACKS, LANES, POINTS = 2, 32, 200
# A leak shows as memory that grows while the big song plays, scrolls and zooms: what it holds
# after SHORT seconds against what it holds after LONG, in one run.
SHORT, LONG = 10, 120
COLOURS = ["blue", "green", "purple", "orange", "pink", "teal", "yellow", "red"]


def run(program, *arguments, check=True):
    """Runs `sound-tools`. Returns its text, (cpu seconds, peak MB) of the process itself, and the
    peak MB of it and of its Bun, added."""
    process = subprocess.Popen([program, *map(str, arguments)], env=ENV, stdout=subprocess.PIPE,
                               stderr=subprocess.STDOUT, text=True)
    exits = None
    if MAC:
        # Told when it ends, before it is reaped: its counts go with it.
        exits = select.kqueue()
        exits.control([select.kevent(process.pid, select.KQ_FILTER_PROC, select.KQ_EV_ADD, select.KQ_NOTE_EXIT)], 0)
    bun, ended = [0.0], threading.Event()
    sampler = threading.Thread(target=sample_bun, args=(process.pid, ended, bun), daemon=True)
    sampler.start()
    text = process.stdout.read()
    peak = None
    if exits:
        exits.control(None, 1)
        peak = footprint(process.pid)
    ended.set()
    sampler.join()
    _, status, usage = os.wait4(process.pid, 0)
    if check and status != 0:
        raise SystemExit(f"sound-tools {' '.join(map(str, arguments))} failed:\n{text}")
    peak = peak or usage.ru_maxrss * MAXRSS_BYTES / 2**20
    return text, (usage.ru_utime + usage.ru_stime, peak), peak + bun[0]


def footprint(pid, at=LIFETIME_MAX_FOOTPRINT):
    """The most memory `pid` held, or what it holds now with `at=FOOTPRINT`, in MB, as macOS
    counts it and `/usr/bin/time -l` says: what is resident and what is compressed. A machine
    short of memory compresses, which the resident size leaves out."""
    info = ctypes.create_string_buffer(512)
    if LIBPROC.proc_pid_rusage(pid, RUSAGE_INFO_V4, info) != 0:
        return None
    return int.from_bytes(info.raw[at:at + 8], "little") / 2**20


def held_at_marks(program, project, events):
    """What `sound-tools` holds now, in MB, as its script passes each mark, by the mark's name."""
    script = ROOT / "leak.json"
    script.write_text(json.dumps(events))
    process = subprocess.Popen([program, project, "--script", script, "--realtime"], env=ENV,
                               stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    held = {}
    for line in process.stdout:
        if line.startswith("rune-mark "):
            held[line.split()[1]] = footprint(process.pid, FOOTPRINT)
    process.wait()
    return held


def sample_bun(pid, ended, peak):
    """The most the children of `pid` held, in MB, looked at 5 times a second until `ended`."""
    while not ended.is_set():
        listed = subprocess.run(["pgrep", "-P", str(pid)], capture_output=True, text=True).stdout
        for child in map(int, listed.split()):
            held = footprint(child) if MAC else None
            if held is None:
                rss = subprocess.run(["ps", "-o", "rss=", "-p", str(child)], capture_output=True, text=True).stdout
                held = int(rss or 0) / 1024
            peak[0] = max(peak[0], held)
        time.sleep(0.2)


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


def lanes(program, folder):
    """The DAW template with LANE_TRACKS tracks of LANES lanes of POINTS points, every lane shown:
    the view is changed to show them as it opens, which only the heads' toggles do."""
    new_project(program, folder)
    view = folder / "extensions/song.tsx"
    opening = "    const page = pageOf(self, ctx);"
    view.write_text(view.read_text().replace(
        opening, f"    if (!memory.opened) memory.lanes = self.children.map((each) => each.name);\n{opening}"))
    numbers = ["position", "effect_amount", "gain", "pan", "detune_cents", "semitone", "octave"]
    targets = [f"synth.{osc}.{number}" for osc in ("osc_1", "osc_2") for number in numbers]
    targets += ["synth.sub.gain", "synth.unison.amount", "synth.lfo_1.rate_hz", "synth.lfo_2.rate_hz",
                "synth.voicing.glide_seconds", "synth.env_3.sustain", "mix.pan", "mix.gain_db"]
    targets += [f"synth.filter_{k}.{n}" for k in (1, 2) for n in ("cutoff_hz", "resonance", "drive_db")]
    points = [{"tick": k * 480, "value": 0.25 if k % 2 == 0 else 0.75} for k in range(POINTS)]
    for row in range(LANE_TRACKS):
        track = folder / f"state/song/t{row:02}"
        if not track.exists():
            shutil.copytree(folder / "state/song/t00", track)
        (track / "synth.json").write_text(json.dumps({"tool": "wavetable", "state": {}}))
        automation = [{"lane": f"lane-{k}", "target": target} for k, target in enumerate(targets[:LANES])]
        (track / "instance.json").write_text(json.dumps({"tool": "track", "state": {
            "title": f"Track {row + 1}", "colour": COLOURS[row], "automation": automation}}))
        for k in range(LANES):
            (track / f"lane-{k}.json").write_text(json.dumps({"tool": "lane", "state": {"points": points, "scale": "linear"}}))


def scenarios(first_clip_bars):
    """Each scenario's script: events, the last at SECONDS."""
    clip = {"x": TIMELINE + first_clip_bars * BAR_POINTS / 2, "y": FIRST_ROW}
    steps = [0.2 + k / 60 for k in range(120)]
    end = [{"t": SECONDS, "type": "mark", "name": "end"}]
    mixer = {"t": 0.2, "type": "tap", "key": "mixer-toggle"}
    return {
        "idle": end,
        "play": [{"t": 0.2, "type": "tap", "key": "play"}, *end],
        "scroll": [{"t": t, "type": "wheel", "key": "song", "dx": -6, "dy": 0} for t in steps] + end,
        "zoom": [{"t": t, "type": "wheel", "key": "song", "dx": 0, "dy": -2, "cmd": True} for t in steps] + end,
        "drag a clip": [{"t": 0.2, "type": "drag", **clip, "dx": 240, "dy": 64, "dur": 2.0}, *end],
        "note editor": [{"t": 0.2, "type": "double-click", **clip}, *end],
        "mixer playing": [mixer, {"t": 0.5, "type": "tap", "key": "play"}, *end],
        "drag a fader": [mixer, {"t": 0.5, "type": "drag", "key": "fader-t00", "dx": 0, "dy": -80, "dur": 2.0}, *end],
    }


def lane_scenarios():
    """The lanes' scripts: a press on a lane opens it, and then its points take the pointer."""
    steps = [0.2 + k / 60 for k in range(120)]
    end = [{"t": SECONDS, "type": "mark", "name": "end"}]
    return {
        "idle": end,
        "scroll": [{"t": t, "type": "wheel", "key": "song", "dx": 0, "dy": -6} for t in steps] + end,
        "zoom": [{"t": t, "type": "wheel", "key": "song", "dx": 0, "dy": -2, "cmd": True} for t in steps] + end,
        "open a lane": [{"t": 0.2, "type": "tap", "key": "lane-t00-lane-0"}, *end],
    }


def played_moving(seconds):
    """The song plays for `seconds`, scrolled and zoomed back and forth every second."""
    moves = []
    for k in range(int(seconds) - 1):
        back = 1 if k % 4 < 2 else -1
        wheel = {"dx": 0, "dy": -20 * back, "cmd": True} if k % 2 else {"dx": -60 * back, "dy": 0}
        moves.append({"t": 0.5 + k, "type": "wheel", "key": "song", **wheel})
    return [{"t": 0.2, "type": "tap", "key": "play"}, *moves, {"t": SHORT, "type": "mark", "name": "short"},
            {"t": seconds, "type": "mark", "name": "end"}]


FRAMES = re.compile(r"rune: (\d+) frames at .* frame cpu mean ([0-9.]+) ms p50 ([0-9.]+) p95 ([0-9.]+) max ([0-9.]+)")
FIRST = re.compile(r"first frame ([0-9.]+) ms")


def measure(program, project, name, events, runs):
    """Rune's frame times of a script, and the process's CPU and memory. Its picture: `<project>-<name>.png`."""
    script = ROOT / f"{project.name}-{name.replace(' ', '-')}.json"
    script.write_text(json.dumps(events))
    rows = []
    for _ in range(runs):
        # Frames at the pace of a window: unpaced, they outrun the GPU, whose queue then grows.
        text, (cpu, mb), total = run(program, project, "--script", script, "--realtime", "--snap", script.with_suffix(".png"))
        found, first = FRAMES.search(text), FIRST.search(text)
        if not found or not first:
            raise SystemExit(f"{name}: no frame times in\n{text}")
        frames, mean, p50, p95, peak = found.groups()
        rows.append({"frames": int(frames), "first ms": float(first.group(1)), "mean ms": float(mean),
                     "p50 ms": float(p50), "p95 ms": float(p95), "max ms": float(peak), "cpu s": cpu, "MB": mb,
                     "with Bun MB": total})
    return {key: statistics.median(row[key] for row in rows) for key in rows[0]}


def played(program, project, seconds, runs):
    """The app's own `--play`: the song from the start on the offline engine, one picture at the end."""
    picture = ROOT / f"{project.name}-play-{seconds}.png"
    rows = [run(program, project, "--snap", picture, "--play", seconds, "--realtime") for _ in range(runs)]
    return {"cpu s": statistics.median(cpu for _, (cpu, _), _ in rows),
            "MB": statistics.median(mb for _, (_, mb), _ in rows),
            "with Bun MB": statistics.median(total for _, _, total in rows)}


def show(report, against):
    columns = ["frames", "first ms", "p50 ms", "p95 ms", "cpu s", "MB", "with Bun MB"]
    print(f"\nHeadless, {SECONDS:.0f} s of frames at 60 fps as a window paces them, median of {report['runs']} runs, load average "
          f"{report['load']:.0f}. Frame ms: Rune's wall time of a frame's work (poll, view, layout, encode); "
          "first: the first frame, which waits for Bun's views. cpu s and MB: the sound-tools process, MB its "
          "peak footprint; with Bun MB: that and Bun's peak.\n")
    print("| project | scenario | " + " | ".join(columns) + " |")
    print("| --- | --- |" + " ---: |" * len(columns))
    for project, by_scenario in report["projects"].items():
        for name, now in by_scenario.items():
            then = (against or {}).get("projects", {}).get(project, {}).get(name, {})
            cells = []
            for column in columns:
                value, old = now.get(column), then.get(column)
                whole = column in ("frames", "first ms") or "MB" in column
                cell = "" if value is None else f"{value:.0f}" if whole else f"{value:.2f}"
                if value is not None and old:
                    cell += f" ({(value - old) / old * 100:+.0f}%)"
                cells.append(cell)
            print(f"| {project} | {name} | " + " | ".join(cells) + " |")
    leak = report["leak"]
    if leak["grew"] is not None:
        print(f"\nThe big song playing, scrolled and zoomed holds {leak['short MB']:.0f} MB after {SHORT} s and "
              f"{leak['long MB']:.0f} MB after {LONG} s ({leak['grew'] * 100:+.1f}%).")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--program", help="a sound-tools to measure; by default a release build of this checkout")
    parser.add_argument("--runs", type=int, default=3)
    parser.add_argument("--against", type=Path, default=BASELINE, help="a saved run to compare with")
    parser.add_argument("--save", action="store_true", help=f"keep this run as {BASELINE.relative_to(REPO)}")
    arguments = parser.parse_args()
    (ROOT / "home").mkdir(parents=True, exist_ok=True)
    program = program_of(arguments.program)
    projects = {"daw": (ROOT / "daw", new_project, scenarios(8)),
                "big song": (ROOT / "big", big_song, scenarios(CLIP_BARS)),
                "lanes": (ROOT / "lanes", lanes, lane_scenarios())}
    report = {"program": run(program, "--version")[0].strip(), "machine": platform.platform(),
              "date": time.strftime("%Y-%m-%d"), "runs": arguments.runs, "projects": {}}
    report["load"] = os.getloadavg()[0]
    for project, (folder, make, scripts) in projects.items():
        print(f"measuring {project}", file=sys.stderr, flush=True)
        make(program, folder)
        results = {name: measure(program, folder, name, events, arguments.runs) for name, events in scripts.items()}
        if project != "lanes":
            results["--play 60"] = played(program, folder, 60, arguments.runs)
        report["projects"][project] = results
    print("measuring a leak", file=sys.stderr, flush=True)
    held = held_at_marks(program, ROOT / "big", played_moving(LONG)) if MAC else {}
    short, long = held.get("short"), held.get("end")
    report["leak"] = {"short MB": short, "long MB": long, "grew": (long - short) / short if short and long else None}
    against = json.loads(arguments.against.read_text()) if arguments.against.exists() else None
    show(report, against)
    out = BASELINE if arguments.save else ROOT / f"{time.strftime('%Y%m%d-%H%M%S')}.json"
    out.write_text(json.dumps(report, indent=1) + "\n")
    print(f"\nwrote {out}", file=sys.stderr)


if __name__ == "__main__":
    main()
