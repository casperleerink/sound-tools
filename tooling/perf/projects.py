"""The benchmark projects. Each is made by the runtime of the commit under test, then filled with
state files and the tool files of that same commit."""

import json
import math
import shutil
import struct
import subprocess
import wave
from pathlib import Path

HERE = Path(__file__).resolve().parent
BAR = 3840  # ticks in a 4/4 bar; at the default 120 bpm a bar is 2 s
BARS = 32  # 64 s, longer than the longest render

# Example tools: file in the repo, state, whether it plays the notes of a clip.
TOOLS = {
    "acid-bass": ("tooling/agent-eval/fixtures/acid-bass.tsx",
                  {"steps": [1, 0, 1, 1, 0, 1, 0, 1, 1, 0, 1, 0, 0, 1, 1, 0],
                   "accents": [1, 0, 0, 0, 0, 1, 0, 0, 1, 0, 0, 0, 0, 0, 1, 0]}, False),
    "dorian-drift": ("tooling/agent-eval/examples/dorian-drift.tsx", {}, False),
    "gravity-harp": ("tooling/agent-eval/examples/gravity-harp.tsx", {}, False),
    "storm": ("tooling/agent-eval/examples/storm.tsx", {}, False),
    "life": ("tooling/agent-eval/examples/life.tsx", {}, True),
    "grain-cloud": ("tooling/agent-eval/examples/grain-cloud.ts", {"sound": "texture.wav"}, False),
}
# TypeScript rebuilds of built-in instruments: rebuild tool -> built-in tool id.
REBUILDS = {"ts-synth": "instrument.synth"}
PATTERNS = ("arp", "chords")


def names():
    """Every project, in the order the tables show them."""
    return (["builtin12", *[f"one-{tool}" for tool in TOOLS], "mixed-base", "mixed", "mixed-x2"]
            + [f"{tool}-{pattern}" for built in REBUILDS.items() for tool in (built[1], built[0])
               for pattern in PATTERNS])


def notes(kind, index):
    root = 36 + (index * 5) % 24
    if kind == "chords":  # four held notes a bar
        return [{"start": bar * BAR, "length": BAR - 40, "pitch": root + 12 + step + (bar % 4) * 2, "velocity": 80}
                for bar in range(BARS) for step in (0, 4, 7, 11)]
    if kind == "arp":  # sixteenths
        return [{"start": step * 240, "length": 200, "pitch": root + 24 + (0, 3, 7, 10, 12, 10, 7, 3)[step % 8],
                 "velocity": 90} for step in range(BARS * 16)]
    return [{"start": step * 480, "length": 400, "pitch": root + (0, 0, 7, 5)[step % 4], "velocity": 100}
            for step in range(BARS * 8)]  # bass in eighths


def write(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data, indent=1))


def track(folder, name, order, instrument, clip_notes, effects=True):
    base = folder / "state/arrangement" / name
    state = {"name": name, "colour": "teal", "order": order}
    if effects:
        state["effects"] = ["reverb", "delay"]
        write(base / "reverb.json", {"tool": "reverb", "state": {}})
        write(base / "delay.json", {"tool": "delay", "state": {}})
    write(base / "instance.json", {"tool": "arrangement.track", "state": state})
    write(base / "instrument.json", instrument)
    if clip_notes is not None:
        write(base / "clip.json", {"tool": "arrangement.clip", "state": {"start": 0, "length": BAR * BARS, "notes": clip_notes}})


def builtins(folder, synths, wavetables, start=0):
    kinds = ("chords", "arp", "bass")
    for i in range(synths):
        track(folder, f"synth{start + i}", start + i, {"tool": "instrument.synth", "state": {"gain": 0.08}},
              notes(kinds[i % 3], i))
    for i in range(wavetables):
        track(folder, f"wave{start + i}", start + synths + i, {"tool": "wavetable", "state": {}},
              notes(kinds[(i + 1) % 3], i + 3))
    return start + synths + wavetables


def texture(folder):
    """A 3 s chord that decays, for grain-cloud to read."""
    path = folder / "assets/audio/texture.wav"
    path.parent.mkdir(parents=True, exist_ok=True)
    with wave.open(str(path), "wb") as out:
        out.setnchannels(1)
        out.setsampwidth(2)
        out.setframerate(44100)
        out.writeframes(b"".join(
            struct.pack("<h", int(32767 * 0.15 * math.exp(-0.3 * t / 44100)
                                  * sum(math.sin(math.tau * hz * t / 44100) for hz in (220, 277.2, 329.6, 440))))
            for t in range(3 * 44100)))


def tool(folder, source, name, order, suffix=""):
    """Adds an example tool on a track. `source` is the checkout of the commit."""
    file, state, plays_notes = TOOLS[name]
    (folder / "extensions").mkdir(exist_ok=True)
    shutil.copy(source / file, folder / "extensions" / Path(file).name)
    track(folder, f"{name}{suffix}", order, {"tool": name, "state": state}, notes("arp", order) if plays_notes else None)


def rebuild_file(source, name):
    """The rebuild tool of the commit, or the harness's own for a commit from before it."""
    own = source / "tooling/perf/tools" / f"{name}.ts"
    return own if own.exists() else HERE / "tools" / f"{name}.ts"


def mixed(folder, source, copies):
    order = 0
    for copy in range(copies):
        order = builtins(folder, 4, 2, start=order)
        for name in TOOLS:
            tool(folder, source, name, order, suffix=f"-{copy}" if copies > 1 else "")
            order += 1


def make(runtime, source, folder, name, env):
    """Makes project `name` in `folder` with `runtime`, from the sources of `source`."""
    shutil.rmtree(folder, ignore_errors=True)
    folder.mkdir(parents=True)
    made = subprocess.run([runtime, folder, "--headless"], input="quit\n", env=env, capture_output=True, text=True)
    if made.returncode != 0 or not (folder / "project.json").exists():
        raise SystemExit(f"could not make {name}:\n{made.stdout}{made.stderr}")
    if name == "builtin12":
        builtins(folder, 8, 4)
    elif name == "mixed-base":
        builtins(folder, 4, 2)
    elif name in ("mixed", "mixed-x2"):
        texture(folder)
        mixed(folder, source, 2 if name == "mixed-x2" else 1)
    elif name.startswith("one-"):
        texture(folder)
        tool(folder, source, name[4:], 0)
    else:  # <instrument>-<pattern>: one instrument, no effects
        instrument, pattern = name.rsplit("-", 1)
        if instrument in REBUILDS:
            (folder / "extensions").mkdir()
            shutil.copy(rebuild_file(source, instrument), folder / "extensions")
        track(folder, "t", 0, {"tool": instrument, "state": {}}, notes(pattern, 0), effects=False)


def set_master_gain(folder, gain_db):
    path = folder / "state/arrangement/instance.json"
    record = json.loads(path.read_text())
    record["state"]["master"]["gain_db"] = gain_db
    path.write_text(json.dumps(record, indent=1))
