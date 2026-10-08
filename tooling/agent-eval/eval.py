"""Gives the app's agent a task in a fresh project and checks what it did.

    python3 tooling/agent-eval/eval.py <scenario> [--runs n]

Each run starts from a new project made by the runtime (as the app makes one), with the files of
the scenario, then runs Claude Code with the app's flags (`claude_run.py`), then checks the
project. It prints one JSON line per run: whether the check passed, what it cost, how many
turns, and which docs the agent opened. Logs go to /tmp/sound-tools-eval/.

Earlier experiments, with their results next to this file:
- `results-docs-vs-skills.jsonl`: the docs map against the docs as Claude Code skills. The map
  did as well or better, without the settings skills need.
- `results-hum-vs-graph.jsonl`: a tool's sound written in Hum with a template against a graph of
  signals built with the SDK's functions. Both passed every task; the graph cost about 10% less
  and needs no doc of its own, so the SDK is the graph alone.
- `results-experiments.jsonl`: a gravity harp page and a generative source built from scratch,
  and a composer that edits the pattern of a builder's acid bass. Every run passed.
"""

import argparse
import json
import re
import shutil
import subprocess
from pathlib import Path

import claude_run

LOGS = Path("/tmp/sound-tools-eval")
HERE = Path(__file__).resolve().parent


def runtime(folder: Path, *arguments: str, stdin: str | None = None) -> str:
    process = subprocess.run([str(claude_run.RUNTIME), str(folder), *arguments], input=stdin,
                             capture_output=True, text=True, timeout=300)
    return process.stdout + process.stderr


def write(folder: Path, relative: str, contents) -> None:
    path = folder / relative
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(contents if isinstance(contents, str) else json.dumps(contents))


def piece(folder: Path) -> None:
    """A pad with four chords and a lead with a melody, both on the built-in synth."""
    write(folder, "state/arrangement/pad/instance.json",
          {"tool": "arrangement.track", "state": {"name": "Pad", "colour": "teal", "order": 0}})
    write(folder, "state/arrangement/lead/instance.json",
          {"tool": "arrangement.track", "state": {"name": "Lead", "colour": "peach", "order": 1}})
    write(folder, "state/arrangement/pad/instrument.json",
          {"tool": "instrument.synth", "state": {"gain": 0.12}})
    write(folder, "state/arrangement/lead/instrument.json",
          {"tool": "instrument.synth", "state": {"gain": 0.15}})
    chords = [[48, 55, 64], [45, 52, 60], [41, 48, 57], [43, 50, 59]]
    notes = [{"start": i * 3840, "length": 3800, "pitch": p, "velocity": 90}
             for i, chord in enumerate(chords) for p in chord]
    write(folder, "state/arrangement/pad/chords.json",
          {"tool": "arrangement.clip", "state": {"start": 0, "length": 15360, "notes": notes}})
    melody = [72, 74, 76, 79, 76, 74, 72, 71, 72, 74, 76, 72, 69, 71, 72, 72]
    notes = [{"start": i * 960, "length": 900, "pitch": p, "velocity": 100}
             for i, p in enumerate(melody)]
    write(folder, "state/arrangement/lead/melody.json",
          {"tool": "arrangement.clip", "state": {"start": 0, "length": 15360, "notes": notes}})


def record(folder: Path, relative: str) -> dict:
    path = folder / relative
    return json.loads(path.read_text()) if path.exists() else {}


def track_effects(folder: Path, track: str) -> list:
    """The tools of the effects of a track, in order, from its records."""
    instance = record(folder, f"state/arrangement/{track}/instance.json")
    effects = instance.get("state", {}).get("effects", [])
    names = [effect if isinstance(effect, str) else effect.get("name") for effect in effects]
    return [record(folder, f"state/arrangement/{track}/{name}.json").get("tool") for name in names]


def no_problems(folder: Path) -> bool:
    return "problems: 0" in runtime(folder, "--inspect")


# Each scenario: what is in the project before, what the composer asks, and the check.

def setup_builtin(folder: Path) -> None:
    piece(folder)


def check_builtin(folder: Path) -> dict:
    lead = track_effects(folder, "lead")
    synth = record(folder, "state/arrangement/pad/instrument.json").get("state", {})
    brighter = synth.get("filter", {}) != {} or any(
        tool in ("eq", "filter") for tool in track_effects(folder, "pad")) or \
        json.dumps(synth) != json.dumps({"gain": 0.12})
    return {"delay on lead": "delay" in lead, "pad changed": brighter,
            "no problems": no_problems(folder)}


TAPE_TOOL = (HERE / "fixtures" / "worn-tape.ts").read_text()


def setup_tape_user(folder: Path) -> None:
    piece(folder)
    write(folder, "extensions/worn-tape.ts", TAPE_TOOL)
    write(folder, "state/arrangement/pad/tape.json",
          {"tool": "worn-tape", "state": {"wear": 0.5, "character": "cassette"}})
    pad = record(folder, "state/arrangement/pad/instance.json")
    pad["state"]["effects"] = ["tape"]
    write(folder, "state/arrangement/pad/instance.json", pad)


def check_tape_user(folder: Path) -> dict:
    lead = track_effects(folder, "lead")
    lead_tape = [name for name in (folder / "state/arrangement/lead").glob("*.json")
                 if record(folder, str(name.relative_to(folder))).get("tool") == "worn-tape"]
    lead_state = record(folder, str(lead_tape[0].relative_to(folder)))["state"] if lead_tape else {}
    pad_state = record(folder, "state/arrangement/pad/tape.json").get("state", {})
    return {
        "tape on lead": "worn-tape" in lead,
        "lead is a light reel": lead_state.get("character") == "reel"
        and lead_state.get("wear", 0.5) < 0.5,
        "pad fully worn": pad_state.get("wear") == 1,
        "no problems": no_problems(folder),
    }


def setup_tape_builder(folder: Path) -> None:
    piece(folder)


def setup_with_bass(folder: Path) -> None:
    piece(folder)
    write(folder, "state/arrangement/bass/instance.json",
          {"tool": "arrangement.track", "state": {"name": "Bass", "colour": "red", "order": 2}})
    write(folder, "state/arrangement/bass/instrument.json",
          {"tool": "instrument.synth", "state": {}})


def project_tools(folder: Path) -> dict:
    """Each tool the project defines, by name, with the text of its file."""
    tools = {}
    for path in (folder / "extensions").glob("*.ts*"):
        if path.name == "sdk.ts":
            continue
        text = path.read_text()
        for name in re.findall(r'name:\s*"([a-z0-9_-]+)"', text):
            tools[name] = text
    return tools


def instruments(folder: Path) -> dict:
    """The tool of the instrument of each track, by track folder."""
    found = {}
    for track in (folder / "state/arrangement").iterdir():
        if track.is_dir():
            found[track.name] = record(folder, f"state/arrangement/{track.name}/instrument.json").get("tool")
    return found


def sounds(folder: Path, track: str) -> bool:
    """Whether the track plays anything in the first eight seconds."""
    report = runtime(folder, "--analyze", "--seconds", "8", "--solo", track)
    match = re.search(r"loudness (-?[0-9.]+) LUFS", report)
    return bool(match) and float(match.group(1)) > -60


def check_sequencer(folder: Path) -> dict:
    tools = project_tools(folder)
    tool = instruments(folder).get("bass")
    text = tools.get(tool, "")
    return {
        "a project tool plays the bass": tool in tools,
        "with a pattern": "pattern(" in text,
        "a step light": "watch" in text and "Steps" in text,
        "it sounds": sounds(folder, "bass"),
        "no problems": no_problems(folder),
    }


def check_toy(folder: Path) -> dict:
    tools = project_tools(folder)
    played = [(track, tool) for track, tool in instruments(folder).items() if tool in tools]
    text = tools.get(played[0][1], "") if played else ""
    return {
        "a project tool plays a track": bool(played),
        "live controls and a trigger": "live(" in text and "trigger(" in text,
        "an XY pad": "Pad" in text,
        "it sounds": bool(played) and sounds(folder, played[0][0]),
        "no problems": no_problems(folder),
    }


BUILT_IN = {"delay", "eq", "filter", "reverb", "compressor", "limiter", "saturator", "utility",
            "modulation", "script", None}


def check_tape_builder(folder: Path) -> dict:
    tools = list((folder / "extensions").glob("*.ts")) if (folder / "extensions").exists() else []
    tools = [path for path in tools if path.name != "sdk.ts"]
    pad = track_effects(folder, "pad")
    return {
        "a tool file": bool(tools),
        "a project tool on the pad": any(tool not in BUILT_IN for tool in pad),
        "no problems": no_problems(folder),
    }


ACID_BASS = (HERE / "fixtures" / "acid-bass.tsx").read_text()


def setup_acid_bass(folder: Path) -> None:
    setup_with_bass(folder)
    write(folder, "extensions/acid-bass.tsx", ACID_BASS)
    write(folder, "state/arrangement/bass/instrument.json", {"tool": "acid-bass", "state": {
        "steps": [1, 0, 1, 1, 0, 1, 1, 0, 1, 0, 1, 1, 0, 1, 0, 1],
        "accents": [1, 0, 0, 1, 0, 0, 1, 0, 0, 0, 1, 0, 0, 1, 0, 0],
        "cutoff": 380, "resonance": 0.7, "note": 36}})


def check_acid_bass(folder: Path) -> dict:
    state = record(folder, "state/arrangement/bass/instrument.json").get("state", {})
    accents = state.get("accents", [])
    return {
        "eighths": state.get("steps") == [1, 0] * 8,
        "accents on the beats": len(accents) == 16 and all(accents[i] == 1 for i in (0, 4, 8, 12)),
        "darker": state.get("cutoff", 380) < 380,
        "an octave lower": state.get("note") == 24,
        "no problems": no_problems(folder),
    }


def check_sweep(folder: Path) -> dict:
    lanes = record(folder, "state/arrangement/bass/instance.json").get("state", {}).get("automation", [])
    cutoff = [lane for lane in lanes if lane.get("parameter") == "cutoff"]
    points = cutoff[0].get("points", []) if cutoff else []
    # Bars 3 and 4 are ticks 7680 to 15360; a lane may hold a value before them.
    inside = [point["value"] for point in points if 7680 <= point.get("tick", -1) <= 15360]
    return {
        "a lane on the cutoff": bool(cutoff),
        "it rises over bars 3 and 4": len(inside) >= 2 and inside[-1] > inside[0],
        "no problems": no_problems(folder),
    }


def setup_texture(folder: Path) -> None:
    """The piece, and three seconds of a soft chord under `assets/audio/texture.wav`."""
    import math
    import struct
    import wave
    piece(folder)
    path = folder / "assets/audio/texture.wav"
    path.parent.mkdir(parents=True, exist_ok=True)
    with wave.open(str(path), "wb") as file:
        file.setnchannels(1)
        file.setsampwidth(2)
        file.setframerate(44100)
        frames = []
        for frame in range(3 * 44100):
            t = frame / 44100
            value = sum(math.sin(math.tau * hz * t) for hz in (220, 277.2, 329.6, 440)) * 0.15
            frames.append(struct.pack("<h", int(value * 32767 * math.exp(-0.3 * t))))
        file.writeframes(b"".join(frames))


def check_granular(folder: Path) -> dict:
    tools = project_tools(folder)
    played = [(track, tool) for track, tool in instruments(folder).items() if tool in tools]
    text = tools.get(played[0][1], "") if played else ""
    state = record(folder, f"state/arrangement/{played[0][0]}/instrument.json").get("state", {}) if played else {}
    return {
        "a project tool plays a track": bool(played),
        "with a sample field": "sample(" in text,
        "on the recording": "texture.wav" in json.dumps(state) or '"texture.wav"' in text,
        "it sounds": bool(played) and sounds(folder, played[0][0]),
        "no problems": no_problems(folder),
    }


def check_experiment(folder: Path) -> dict:
    tools = project_tools(folder)
    top = [path for path in (folder / "state").glob("*.json")
           if record(folder, str(path.relative_to(folder))).get("tool") in tools]
    name = top[0].stem if top else None
    tool = record(folder, f"state/{name}.json").get("tool") if name else None
    text = tools.get(tool, "")
    connections = json.loads((folder / "project.json").read_text()).get("connections", [])
    return {
        "no arrangement": not (folder / "state/arrangement").exists(),
        "a project tool at the top": bool(top),
        "with a page, a canvas and a tick": all(word in text for word in ("page", "Canvas", "tick")),
        "connected to the device": any(c.get("from", {}).get("instance") == name for c in connections),
        "no problems": no_problems(folder),
    }


def check_generative(folder: Path) -> dict:
    tools = project_tools(folder)
    tool = instruments(folder).get("pad")
    text = tools.get(tool, "")
    return {
        "a project tool plays the pad": tool in tools,
        "a source": '"source"' in text,
        "shows its note": "watch(" in text,
        "it sounds": sounds(folder, "pad"),
        "no problems": no_problems(folder),
    }


SCENARIOS = {
    "builtin": (setup_builtin, check_builtin,
                "Put a dotted-eighth echo on the lead, quieter than the dry sound, and make the "
                "pad a bit brighter."),
    "tape-user": (setup_tape_user, check_tape_user,
                  "The worn tape sound on the pad is great. Put that same tape effect on the lead "
                  "too, but like a fairly fresh reel tape so it only wobbles a little. And make "
                  "the pad's tape really worn out, the most it goes."),
    "tape-builder": (setup_tape_builder, check_tape_builder,
                     "I want my pad to sound like it's playing off an old worn tape: that slow, "
                     "seasick pitch wobble (wow) plus a faster little flutter, and a bit of tape "
                     "hiss under it. Give me a knob for how worn the tape is, and let me pick "
                     "between a 'cassette' and a 'reel' character; reel should be gentler and a "
                     "bit brighter. Put it on the pad."),
    "sequencer-builder": (setup_with_bass, check_sequencer,
                          "On the bass track I want a 16-step acid bass sequencer as its "
                          "instrument: I click steps on and off on its card and the step that "
                          "is playing lights up, it plays along with the tempo of the piece, "
                          "and it has knobs for the filter cutoff, the resonance and the note "
                          "it plays. Make a pattern that grooves to start with."),
    "experiment-builder": (setup_tape_builder, check_experiment,
                           "Turn this project into an interactive sound toy instead of a song, "
                           "just one screen and no timeline: a 'gravity harp'. I drop balls "
                           "with the mouse onto a few horizontal strings; when a ball hits a "
                           "string it plucks it, each string a different note of a pentatonic "
                           "scale, and the ball bounces off."),
    "generative-builder": (setup_tape_builder, check_generative,
                           "Make the pad play a generative ambient part by itself, forever: it "
                           "slowly wanders between notes of D dorian with a long, washy tail. "
                           "Give its card a way to change how dense and how bright it is, and "
                           "let me see which note it is on."),
    "composer-sequencer": (setup_acid_bass, check_acid_bass,
                           "The acid bass is cool. Make its pattern play every eighth note and "
                           "nothing in between, put an accent on each beat, and make it a bit "
                           "darker and an octave lower."),
    "granular-builder": (setup_texture, check_granular,
                         "I put a recording in assets/audio/texture.wav. Make a granular cloud "
                         "of it on a new track: it sprays tiny grains of the recording by itself, "
                         "with knobs for grain size, density, where in the file it reads, and how "
                         "much the grains' pitch spreads."),
    "composer-sweep": (setup_acid_bass, check_sweep,
                       "Over bars 3 and 4, sweep the acid bass filter cutoff up from dark to "
                       "bright."),
    "toy-builder": (setup_tape_builder, check_toy,
                    "Make me a 'storm' on a new track: a windy, noisy drone I play live with an "
                    "XY pad, across is how dark or bright, up is how wild, plus a 'thunder' "
                    "button that sets off a deep rumble. It should run by itself, no notes "
                    "needed."),
}


def evaluate(scenario: str, number: int) -> dict:
    setup, check, prompt = SCENARIOS[scenario]
    folder = LOGS / f"{scenario}-{number}"
    shutil.rmtree(folder, ignore_errors=True)
    folder.mkdir(parents=True)
    # As the app makes a project, and writes its docs while it has it open.
    runtime(folder, "--headless", stdin="quit\n")
    setup(folder)
    # The window makes this folder for every project it opens.
    (folder / "extensions").mkdir(exist_ok=True)
    runtime(folder, "--headless", stdin="quit\n")
    run = claude_run.run(folder, prompt, log=LOGS / f"{folder.name}.log")
    checks = check(folder)
    return {"scenario": scenario, "run": number,
            "passed": all(checks.values()), "checks": checks, **run.summary()}


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("scenario", choices=SCENARIOS)
    parser.add_argument("--runs", type=int, default=1)
    arguments = parser.parse_args()
    LOGS.mkdir(exist_ok=True)
    for number in range(1, arguments.runs + 1):
        result = evaluate(arguments.scenario, number)
        print(json.dumps(result), flush=True)
        with open(LOGS / "results.jsonl", "a") as results:
            results.write(json.dumps(result) + "\n")


if __name__ == "__main__":
    main()
