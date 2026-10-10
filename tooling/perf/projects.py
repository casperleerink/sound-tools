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
    return (["builtin12", "automated", *[f"one-{tool}" for tool in TOOLS], "mixed-base", "mixed", "mixed-x2"]
            + [f"{tool}-{pattern}" for built in REBUILDS.items() for tool in (built[1], built[0])
               for pattern in PATTERNS])


def notes(kind, index, bars=BARS):
    root = 36 + (index * 5) % 24
    if kind == "chords":  # four held notes a bar
        return [{"start": bar * BAR, "length": BAR - 40, "pitch": root + 12 + step + (bar % 4) * 2, "velocity": 80}
                for bar in range(bars) for step in (0, 4, 7, 11)]
    if kind == "arp":  # sixteenths
        return [{"start": step * 240, "length": 200, "pitch": root + 24 + (0, 3, 7, 10, 12, 10, 7, 3)[step % 8],
                 "velocity": 90} for step in range(bars * 16)]
    return [{"start": step * 480, "length": 400, "pitch": root + (0, 0, 7, 5)[step % 4], "velocity": 100}
            for step in range(bars * 8)]  # bass in eighths


def write(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data, indent=1))


def drums(bars=BARS):
    """Kick on 1 and 3, snare on 2 and 4, closed hat on every eighth: notes of the Drum pad."""
    hits = [(step * 480, 42) for step in range(bars * 8)]
    hits += [(beat * 960, 36 if beat % 2 == 0 else 38) for beat in range(bars * 4)]
    return [{"start": start, "length": 120, "pitch": pitch, "velocity": 100} for start, pitch in sorted(hits)]


def track(folder, name, order, instrument, clip_notes, effects=None, gain_db=0.0, bars=BARS):
    """An instrument track. `effects` is effect name -> state, reverb and delay at their defaults
    when left out."""
    base = folder / "state/arrangement" / name
    state = {"name": name, "colour": "teal", "order": order}
    if gain_db:
        state["gain_db"] = gain_db
    effects = {"reverb": {}, "delay": {}} if effects is None else effects
    if effects:
        state["effects"] = list(effects)
        for effect, effect_state in effects.items():
            write(base / f"{effect}.json", {"tool": effect, "state": effect_state})
    write(base / "instance.json", {"tool": "arrangement.track", "state": state})
    write(base / "instrument.json", instrument)
    if clip_notes is not None:
        clip = {"start": 0, "length": BAR * bars, "notes": clip_notes}
        write(base / "clip.json", {"tool": "arrangement.clip", "state": clip})


def builtins(folder, synths, wavetables, start=0):
    kinds = ("chords", "arp", "bass")
    for i in range(synths):
        track(folder, f"synth{start + i}", start + i, {"tool": "instrument.synth", "state": {"gain": 0.08}},
              notes(kinds[i % 3], i))
    for i in range(wavetables):
        track(folder, f"wave{start + i}", start + synths + i, {"tool": "wavetable", "state": {}},
              notes(kinds[(i + 1) % 3], i + 3))
    return start + synths + wavetables


def write_wav(folder, name, samples, rate):
    """A mono 16-bit file under assets/audio/."""
    path = folder / "assets/audio" / name
    path.parent.mkdir(parents=True, exist_ok=True)
    with wave.open(str(path), "wb") as out:
        out.setnchannels(1)
        out.setsampwidth(2)
        out.setframerate(rate)
        out.writeframes(b"".join(struct.pack("<h", int(32767 * sample)) for sample in samples))


def texture(folder):
    """A 3 s chord that decays, for grain-cloud to read."""
    write_wav(folder, "texture.wav", (0.15 * math.exp(-0.3 * t / 44100)
                                      * sum(math.sin(math.tau * hz * t / 44100) for hz in (220, 277.2, 329.6, 440))
                                      for t in range(3 * 44100)), 44100)


def tool(folder, source, name, order, suffix=""):
    """Adds an example tool on a track. `source` is the checkout of the commit."""
    file, state, plays_notes = TOOLS[name]
    (folder / "extensions").mkdir(exist_ok=True)
    shutil.copy(source / file, folder / "extensions" / Path(file).name)
    clip_notes = notes("arp", order) if plays_notes else None
    track(folder, f"{name}{suffix}", order, {"tool": name, "state": state}, clip_notes)


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


# Built-in devices. `bi-<device>` is BI_TRACKS tracks of it in a typical use, each with its own notes.
# An effect sits on the synth playing chords, so its own cost is bi-<effect> minus bi-synth.
# `bi-<device>-tail` plays the first bar only: its cost over bi-synth-tail is what the device does
# once its sound has died. `bi-heavy` stresses built-ins; `bi-heavy-idle` is it with no clips.
BI_TRACKS = 4
BI_BARS = 256  # 512 s, so a profile, which renders far ahead of its start, samples the notes
SYNTH = {"tool": "instrument.synth", "state": {"gain": 0.08}}
BI_INSTRUMENTS = {
    "synth": SYNTH,
    "wavetable": {"tool": "wavetable", "state": {}},
    "sampler": {"tool": "sampler", "state": {"sample": "pluck.wav"}},
    "drum-pad": {"tool": "drum-pad", "state": {}},
}
BI_EFFECTS = {
    "reverb": {},
    "delay": {},
    "eq": {"bands": [
        {"shape": "low_cut", "frequency_hz": 80.0},
        {"shape": "bell", "frequency_hz": 400.0, "gain_db": -3.0, "q": 1.5},
        {"shape": "bell", "frequency_hz": 3000.0, "gain_db": 2.0},
        {"shape": "high_shelf", "frequency_hz": 8000.0, "gain_db": 1.5},
    ]},
    "compressor": {"threshold_db": -30.0},
    "limiter": {"gain_db": 12.0},
    "saturator": {},
    "filter": {"cutoff_hz": 1200.0, "resonance": 0.3, "lfo_depth_octaves": 1.0},
    "modulation": {},
    "utility": {"pan": -0.3, "width": 0.8},
}
HEAVY = {"reverb": {}, "delay": {}, "eq": BI_EFFECTS["eq"], "compressor": BI_EFFECTS["compressor"]}
# The reverb settings the IR check renders, all wet only. Default, a long hall, a short room.
REVERBS = {
    "default": {"mix": 1.0},
    "long": {"mix": 1.0, "decay_seconds": 6.0, "size": 1.0, "damping": 0.3},
    "short": {"mix": 1.0, "decay_seconds": 0.6, "size": 0.15, "damping": 0.6},
}
CLICK_SECONDS = 0.1  # where the click is in its file, away from the ramp at the clip's edge
RATE = 48_000  # the rate of an offline render


def builtin_names():
    devices = [*BI_INSTRUMENTS, *BI_EFFECTS]
    return ["empty", *[f"bi-{device}" for device in devices], *[f"bi-{device}-tail" for device in devices],
            "bi-heavy", "bi-heavy-idle"]


def reverb_names():
    return [*[f"rv-ir-{setting}" for setting in REVERBS], "rv-chords", "rv-drums"]


GROUPS = {"builtins": builtin_names, "reverb": reverb_names}


def builtin(folder, device, tail):
    """BI_TRACKS tracks of one built-in device; see BI_TRACKS."""
    if device == "sampler":  # a plucked C4, the sampler's root, that rings for 1.5 s
        write_wav(folder, "pluck.wav", (0.15 * math.exp(-3 * t / RATE)
                                        * sum(math.sin(math.tau * 261.63 * k * t / RATE) / k for k in range(1, 8))
                                        for t in range(int(1.5 * RATE))), RATE)
    instrument = BI_INSTRUMENTS.get(device, SYNTH)
    effects = {device: BI_EFFECTS[device]} if device in BI_EFFECTS else {}
    for index in range(BI_TRACKS):
        played = drums(BI_BARS) if device == "drum-pad" else notes("chords", index, BI_BARS)
        if tail:
            played = [note for note in played if note["start"] < BAR]
        # Four tracks of samples or drums would go over the master's ceiling.
        loud = device in ("sampler", "drum-pad")
        track(folder, f"{device}{index}", index, instrument, played, effects, -12.0 if loud else 0.0, BI_BARS)


def heavy(folder, clips):
    """24 tracks, a synth or a wavetable each, through reverb, delay, EQ and compressor."""
    kinds = ("chords", "arp", "bass")
    for index in range(24):
        instrument = SYNTH if index % 2 == 0 else BI_INSTRUMENTS["wavetable"]
        played = notes(kinds[index % 3], index) if clips else None
        track(folder, f"heavy{index}", index, instrument, played, HEAVY)


def reverb_project(folder, name):
    """rv-ir-<setting>: a click on an audio track into a wet reverb. rv-chords and rv-drums: the
    default reverb on the synth playing chords and on the Drum pad."""
    if name == "rv-chords":
        return track(folder, "chords", 0, SYNTH, notes("chords", 0), {"reverb": {}})
    if name == "rv-drums":
        return track(folder, "drums", 0, BI_INSTRUMENTS["drum-pad"], drums(), {"reverb": {}})
    click = int(CLICK_SECONDS * RATE)
    write_wav(folder, "click.wav", (0.5 if t == click else 0.0 for t in range(click * 2)), RATE)
    base = folder / "state/arrangement/click"
    state = {"name": "click", "colour": "teal", "order": 0, "kind": "audio", "effects": ["reverb"]}
    write(base / "instance.json", {"tool": "arrangement.track", "state": state})
    write(base / "reverb.json", {"tool": "reverb", "state": REVERBS[name.removeprefix("rv-ir-")]})
    write(base / "take.json", {"tool": "arrangement.audio_clip", "state": {"asset": "click.wav", "start": 0}})


def automate(folder):
    """On every track, a gate on its volume and on the cutoff of its instrument each sixteenth: up
    in 5 ticks, then down over the rest, so each lane bends twice a sixteenth inside a block."""
    cutoffs = {"instrument.synth": "cutoff_hz", "wavetable": "filter_1.cutoff_hz"}

    def gate(low, high):
        return [point for step in range(BARS * 16)
                for point in ({"tick": step * 240, "value": low}, {"tick": step * 240 + 5, "value": high})]

    for path in (folder / "state/arrangement").glob("*/instance.json"):
        instrument = path.parent / "instrument.json"
        if not instrument.exists():
            continue
        record = json.loads(path.read_text())
        record["state"]["automation"] = [
            {"parameter": "gain_db", "points": gate(-12.0, 0.0)},
            {"device": "instrument", "parameter": cutoffs[json.loads(instrument.read_text())["tool"]],
             "points": gate(400.0, 4000.0)},
        ]
        write(path, record)


def make(runtime, source, folder, name, env):
    """Makes project `name` in `folder` with `runtime`, from the sources of `source`."""
    shutil.rmtree(folder, ignore_errors=True)
    folder.mkdir(parents=True)
    made = subprocess.run([runtime, folder, "--headless"], input="quit\n", env=env, capture_output=True, text=True)
    if made.returncode != 0 or not (folder / "project.json").exists():
        raise SystemExit(f"could not make {name}:\n{made.stdout}{made.stderr}")
    if name == "builtin12":
        builtins(folder, 8, 4)
    elif name == "automated":
        builtins(folder, 8, 4)
        automate(folder)
    elif name == "mixed-base":
        builtins(folder, 4, 2)
    elif name in ("mixed", "mixed-x2"):
        texture(folder)
        mixed(folder, source, 2 if name == "mixed-x2" else 1)
    elif name == "empty":
        pass
    elif name.startswith("bi-heavy"):
        heavy(folder, clips=name == "bi-heavy")
    elif name.startswith("bi-"):
        builtin(folder, name[3:].removesuffix("-tail"), tail=name.endswith("-tail"))
    elif name.startswith("rv-"):
        reverb_project(folder, name)
    elif name.startswith("one-"):
        texture(folder)
        tool(folder, source, name[4:], 0)
    else:  # <instrument>-<pattern>: one instrument, no effects
        instrument, pattern = name.rsplit("-", 1)
        if instrument in REBUILDS:
            (folder / "extensions").mkdir()
            shutil.copy(rebuild_file(source, instrument), folder / "extensions")
        track(folder, "t", 0, {"tool": instrument, "state": {}}, notes(pattern, 0), effects={})


def set_master_gain(folder, gain_db):
    path = folder / "state/arrangement/instance.json"
    record = json.loads(path.read_text())
    record["state"]["master"]["gain_db"] = gain_db
    path.write_text(json.dumps(record, indent=1))
