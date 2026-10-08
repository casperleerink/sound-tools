"""Gives the app's agent a task in a fresh project and checks what it did.

    python3 tooling/agent-eval/eval.py <scenario> [--variant docs|graph] [--runs n]

Each run starts from a new project made by the runtime (as the app makes one), with the files of
the scenario, then runs Claude Code with the app's flags (`claude_run.py`), then checks the
project. It prints one JSON line per run: whether the check passed, what it cost, how many
turns, and which docs the agent opened. Logs go to /tmp/sound-tools-eval/.

Variants:
- `docs`: the docs as the runtime writes them: a tool's sound is Hum, written with the `hum` tag.
- `graph`: a tool's sound is a graph of signals built with the SDK's functions; the doc of
  writing a tool is `variants/extensions-graph.md` and the Hum doc is gone.

An earlier experiment compared the docs map with the docs as Claude Code skills; its results are
in `results-docs-vs-skills.jsonl`: the map did as well or better, without the settings skills
need.
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
    "toy-builder": (setup_tape_builder, check_toy,
                    "Make me a 'storm' on a new track: a windy, noisy drone I play live with an "
                    "XY pad, across is how dark or bright, up is how wild, plus a 'thunder' "
                    "button that sets off a deep rumble. It should run by itself, no notes "
                    "needed."),
}


def as_graph(folder: Path) -> None:
    """The doc of writing a tool teaches the sound graph, and the Hum doc is gone."""
    docs = folder / "agent-docs"
    generated = (docs / "extensions.md").read_text().split("\n\n", 1)[0]
    graph = (HERE / "variants" / "extensions-graph.md").read_text()
    (docs / "extensions.md").write_text(f"{generated}\n\n{graph}")
    (docs / "hum.md").unlink()
    agents = folder / "AGENTS.md"
    lines = agents.read_text().splitlines(keepends=True)
    agents.write_text("".join(line for line in lines if "agent-docs/hum.md" not in line))


def evaluate(scenario: str, variant: str, number: int) -> dict:
    setup, check, prompt = SCENARIOS[scenario]
    folder = LOGS / f"{scenario}-{variant}-{number}"
    shutil.rmtree(folder, ignore_errors=True)
    folder.mkdir(parents=True)
    # As the app makes a project, and writes its docs while it has it open.
    runtime(folder, "--headless", stdin="quit\n")
    setup(folder)
    # The window makes this folder for every project it opens.
    (folder / "extensions").mkdir(exist_ok=True)
    runtime(folder, "--headless", stdin="quit\n")
    if variant == "graph":
        as_graph(folder)
    run = claude_run.run(folder, prompt, log=LOGS / f"{folder.name}.log")
    checks = check(folder)
    return {"scenario": scenario, "variant": variant, "run": number,
            "passed": all(checks.values()), "checks": checks, **run.summary()}


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("scenario", choices=SCENARIOS)
    parser.add_argument("--variant", choices=["docs", "graph"], default="docs")
    parser.add_argument("--runs", type=int, default=1)
    arguments = parser.parse_args()
    LOGS.mkdir(exist_ok=True)
    for number in range(1, arguments.runs + 1):
        result = evaluate(arguments.scenario, arguments.variant, number)
        print(json.dumps(result), flush=True)
        with open(LOGS / "results.jsonl", "a") as results:
            results.write(json.dumps(result) + "\n")


if __name__ == "__main__":
    main()
