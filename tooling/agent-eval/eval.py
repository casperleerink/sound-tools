"""Gives the app's agent a task in a fresh project and checks what it did.

    python3 tooling/agent-eval/eval.py <scenario>... [--runs n] [--model claude-opus-5-5]
    python3 tooling/agent-eval/eval.py ui      # the scenarios with criteria for the council

Each run starts from a new project made by the runtime (as the app makes one), with the files of
the scenario, then runs Claude Code with the app's flags (`claude_run.py`), one message after
another in one conversation, then checks the project. A scenario in `CRITERIA` then goes to the
council: Opus and GPT, each on a copy of its own, check each criterion by using the result
through the app, score its UI and UX against DESIGN.md, and keep the screenshots that show it.
It prints one JSON line per run: the checks, each message's cost and how the builder checked its
work, and the council's verdicts. `report.py` puts runs side by side by `--label`. Logs go to
~/.cache/sound-tools-eval/, which a restart keeps. `$SOUND_TOOLS_RUNTIME` and
`$SOUND_TOOLS_TEMPLATE` (the DAW template's folder) pick another program and template to compare.

`examples/` holds tools agents built in these runs, from one prompt each: a gravity harp view,
a storm drone played from its display, a generative part in D dorian, and a grain cloud of a sample. Copy one into the
`extensions/` folder of a project to try it.
"""

import argparse
import json
import re
import shutil
import subprocess
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

import claude_run

LOGS = claude_run.LOGS
HERE = Path(__file__).resolve().parent


def runtime(folder: Path, *arguments: str, stdin: str | None = None) -> str:
    process = subprocess.run([str(claude_run.RUNTIME), str(folder), *arguments], input=stdin,
                             capture_output=True, text=True, timeout=300)
    return process.stdout + process.stderr


def write(folder: Path, relative: str, contents) -> None:
    path = folder / relative
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(contents if isinstance(contents, str) else json.dumps(contents))


def track(folder: Path, name: str, title: str, synth: dict, clips: dict) -> None:
    """A track of the song of the DAW template, after the others, with these clips."""
    base = f"{SONG}/{name}"
    write(folder, f"{base}/instance.json", {"tool": "track", "state": {"title": title}})
    write(folder, f"{base}/synth.json", {"tool": "instrument.synth", "state": synth})
    write(folder, f"{base}/chain/instance.json", {"tool": "chain", "state": {}})
    write(folder, f"{base}/mix.json", {"tool": "fader", "state": {}})
    write(folder, f"{base}/rec.json", {"tool": "recorder", "state": {}})
    for clip, notes in clips.items():
        write(folder, f"{base}/{clip}.json", {"tool": "timeline", "state": {"start": 0, "length": 15360, "notes": notes}})
    song = record(folder, f"{SONG}/instance.json")
    song.setdefault("state", {}).setdefault("order", []).append(name)
    write(folder, f"{SONG}/instance.json", song)


def piece(folder: Path) -> None:
    """A new DAW project whose tracks are a pad with four chords and a lead with a melody, both on
    the built-in synth."""
    setup_daw(folder)
    shutil.rmtree(folder / SONG / "t00")
    chords = [[48, 55, 64], [45, 52, 60], [41, 48, 57], [43, 50, 59]]
    notes = [{"start": i * 3840, "length": 3800, "pitch": p, "velocity": 90}
             for i, chord in enumerate(chords) for p in chord]
    track(folder, "pad", "Pad", {"gain": 0.12}, {"chords": notes})
    melody = [72, 74, 76, 79, 76, 74, 72, 71, 72, 74, 76, 72, 69, 71, 72, 72]
    notes = [{"start": i * 960, "length": 900, "pitch": p, "velocity": 100}
             for i, p in enumerate(melody)]
    track(folder, "lead", "Lead", {"gain": 0.15}, {"melody": notes})


def record(folder: Path, relative: str) -> dict:
    path = folder / relative
    return json.loads(path.read_text()) if path.exists() else {}


def track_effects(folder: Path, track: str) -> list:
    """The tools of the effects of a track's chain, in order, from its records."""
    names = record(folder, f"{SONG}/{track}/chain/instance.json").get("state", {}).get("order", [])
    return [record(folder, f"{SONG}/{track}/chain/{name}.json").get("tool") for name in names]


def no_problems(folder: Path) -> bool:
    return "problems: 0" in runtime(folder, "--inspect")


# Each scenario: what is in the project before, what the composer asks, and the check.

def setup_builtin(folder: Path) -> None:
    piece(folder)


def check_builtin(folder: Path) -> dict:
    lead = track_effects(folder, "lead")
    synth = record(folder, f"{SONG}/pad/synth.json").get("state", {})
    brighter = synth.get("cutoff_hz", 2000) > 2000 or any(
        tool in ("eq", "filter") for tool in track_effects(folder, "pad"))
    return {"delay on lead": "delay" in lead, "pad brighter": brighter,
            "no problems": no_problems(folder)}


TAPE_TOOL = (HERE / "fixtures" / "worn-tape.ts").read_text()


def setup_tape_user(folder: Path) -> None:
    piece(folder)
    write(folder, "extensions/worn-tape.ts", TAPE_TOOL)
    write(folder, f"{SONG}/pad/chain/tape.json",
          {"tool": "worn-tape", "state": {"wear": 0.5, "character": "cassette"}})
    write(folder, f"{SONG}/pad/chain/instance.json", {"tool": "chain", "state": {"order": ["tape"]}})


def check_tape_user(folder: Path) -> dict:
    lead = track_effects(folder, "lead")
    lead_tape = [name for name in (folder / SONG / "lead/chain").glob("*.json")
                 if record(folder, str(name.relative_to(folder))).get("tool") == "worn-tape"]
    lead_state = record(folder, str(lead_tape[0].relative_to(folder)))["state"] if lead_tape else {}
    pad_state = record(folder, f"{SONG}/pad/chain/tape.json").get("state", {})
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
    track(folder, "bass", "Bass", {}, {})


def project_tools(folder: Path) -> dict:
    """Each tool the project defines beyond the template's, by name, with the text of its file."""
    template = {path.name for path in (DAW / "extensions").iterdir()}
    tools = {}
    for path in (folder / "extensions").glob("*.ts*"):
        if path.name in template or path.name == "sdk.ts":
            continue
        text = path.read_text()
        for name in re.findall(r'name:\s*"([a-z0-9_-]+)"', text):
            tools[name] = text
    return tools


def instances(folder: Path, owner: str = "") -> dict:
    """The record of every instance under `state/<owner>`, by id."""
    found = {}
    for path in (folder / "state" / owner).rglob("*.json"):
        name = path.relative_to(folder / "state").with_suffix("").as_posix().removesuffix("/instance")
        found[name] = record(folder, str(path.relative_to(folder)))
    return found


def instruments(folder: Path) -> dict:
    """The tool of the instrument of each track, by track folder."""
    found = {}
    for synth in (folder / SONG).glob("*/synth.json"):
        found[synth.parent.name] = record(folder, str(synth.relative_to(folder))).get("tool")
    return found


def sounds(folder: Path, only: str | None = None) -> bool:
    """Whether the project, or the instance `only`, plays anything in the first eight seconds."""
    report = runtime(folder, "--analyze", "--seconds", "8", *(["--only", only] if only else []))
    match = re.search(r"loudness (-?[0-9.]+) LUFS", report)
    return bool(match) and float(match.group(1)) > -60


def check_track(track: str, own_view: bool | None = None):
    """A project tool on `track`, which still plays; with `own_view`, a view of its own, or the
    plain one of its knobs. How a card looks is in the screenshots."""
    def check(folder: Path) -> dict:
        tools = project_tools(folder)
        used = [found.get("tool") for found in instances(folder, f"song/{track}").values()
                if found.get("tool") in tools]
        checks = {"a project tool on the track": bool(used),
                  "it sounds": sounds(folder, f"song/{track}"),
                  "no problems": no_problems(folder)}
        if own_view is not None:
            text = tools.get(used[0], "") if used else ""
            checks["its own view" if own_view else "the plain view"] = bool(used) and ("view:" in text) == own_view
        return checks
    return check


def played_tracks(folder: Path) -> list:
    """The tracks a project tool plays as their instrument."""
    tools = project_tools(folder)
    return [track for track, tool in instruments(folder).items() if tool in tools]


def check_new_track(folder: Path) -> dict:
    played = played_tracks(folder)
    return {"a project tool plays a track": bool(played),
            "it sounds": bool(played) and sounds(folder, f"song/{played[0]}"),
            "no problems": no_problems(folder)}


ACID_BASS = (HERE / "fixtures" / "acid-bass.tsx").read_text()


def setup_acid_bass(folder: Path) -> None:
    setup_with_bass(folder)
    write(folder, "extensions/acid-bass.tsx", ACID_BASS)
    write(folder, f"{SONG}/bass/synth.json", {"tool": "acid-bass", "state": {
        "steps": [1, 0, 1, 1, 0, 1, 1, 0, 1, 0, 1, 1, 0, 1, 0, 1],
        "accents": [1, 0, 0, 1, 0, 0, 1, 0, 0, 0, 1, 0, 0, 1, 0, 0],
        "cutoff": 380, "resonance": 0.7, "note": 36}})


def check_acid_bass(folder: Path) -> dict:
    state = record(folder, f"{SONG}/bass/synth.json").get("state", {})
    accents = state.get("accents", [])
    return {
        "eighths": state.get("steps") == [1, 0] * 8,
        "accents on the beats": len(accents) == 16 and all(accents[i] == 1 for i in (0, 4, 8, 12)),
        "darker": state.get("cutoff", 380) < 380,
        "an octave lower": state.get("note") == 24,
        "no problems": no_problems(folder),
    }


def check_sweep(folder: Path) -> dict:
    lanes = record(folder, f"{SONG}/bass/instance.json").get("state", {}).get("automation", [])
    cutoff = [lane["lane"] for lane in lanes if lane.get("target", "").endswith(".cutoff")]
    points = record(folder, f"{SONG}/bass/{cutoff[0]}.json").get("state", {}).get("points", []) if cutoff else []
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
    played = played_tracks(folder)
    text = project_tools(folder).get(instruments(folder)[played[0]], "") if played else ""
    state = record(folder, f"{SONG}/{played[0]}/synth.json").get("state", {}) if played else {}
    return {**check_new_track(folder),
            "on the recording": "texture.wav" in json.dumps(state) or '"texture.wav"' in text}


def connections(folder: Path) -> list:
    return record(folder, "project.json").get("connections", [])


def window(folder: Path) -> str | None:
    """The instance whose view fills the window: `main`, else the first tool of the project at
    the top, by name."""
    tools = project_tools(folder)
    top = sorted(name for name, found in instances(folder).items()
                 if "/" not in name and found.get("tool") in tools)
    return record(folder, "project.json").get("main") or next(iter(top), None)


def check_screen(folder: Path) -> dict:
    """The project turned into one screen: a project tool fills the window and plays to the
    speakers, and the song no longer does."""
    top = {name: found.get("tool") for name, found in instances(folder).items() if "/" not in name}
    main = window(folder)
    heard = [c.get("from", {}).get("instance", "").split("/")[0] for c in connections(folder)
             if "device_output" in c.get("to", {})]
    return {"a project tool fills the window": top.get(main) in project_tools(folder),
            "it plays to the speakers": main in heard,
            "the song is silent": all(top.get(name) != "song" for name in heard),
            "no problems": no_problems(folder)}


def check_drones(folder: Path) -> dict:
    return {**check_screen(folder), "it sounds": sounds(folder)}


def check_mic(folder: Path) -> dict:
    return {**check_screen(folder),
            "hears the input": any("device_input" in c.get("from", {}) for c in connections(folder))}


def check_apps(folder: Path) -> dict:
    return {**check_screen(folder),
            "hears an app": any("app" in c.get("from", {}) for c in connections(folder))}


def check_keys_composer(folder: Path) -> dict:
    # The keys play any track as a MIDI keyboard does: nothing to build or replace.
    return {"keeps the built-in synth": instruments(folder).get("pad") == "instrument.synth",
            "no project tool": not project_tools(folder), "no problems": no_problems(folder)}



# The DAW template: a song of tracks written with the SDK, as "New DAW project" makes it.
DAW = HERE.parents[1] / "templates" / "daw"
SONG = "state/song"


def setup_daw(folder: Path) -> None:
    """The folder as a new DAW project: the files of the template in the project the runtime made,
    which enables every extension."""
    project = json.loads((folder / "project.json").read_text())
    template = json.loads((DAW / "project.json").read_text())
    shutil.copytree(DAW, folder, dirs_exist_ok=True,
                    ignore=shutil.ignore_patterns("template.json", "project.json"))
    write(folder, "project.json", {**project, **template})


def setup_daw_chords(folder: Path) -> None:
    """A new DAW project whose first track holds four chords of a bar each."""
    setup_daw(folder)
    chords = [[48, 55, 64], [45, 52, 60], [41, 48, 57], [43, 50, 59]]
    notes = [{"start": i * 3840, "length": 3800, "pitch": p, "velocity": 90}
             for i, chord in enumerate(chords) for p in chord]
    write(folder, "state/song/t00/chords.json",
          {"tool": "timeline", "state": {"start": 0, "length": 15360, "notes": notes}})


def check_daw_return(folder: Path) -> dict:
    returns = [path.parent.name for path in (folder / "state/song").glob("*/instance.json")
               if record(folder, str(path.relative_to(folder))).get("tool") == "bus"]
    reverb = any(found.get("tool") == "reverb" for name in returns
                 for found in instances(folder, f"song/{name}").values())
    sends = [record(folder, f"state/song/t00/send-{name}.json").get("state", {}) for name in returns]
    # A send at "-inf" is off, and no number.
    gains = [send.get("gain_db", 0) for send in sends if send]
    quiet = any(isinstance(gain, (int, float)) and -40 < gain < 0 for gain in gains)
    return {"a return with a reverb": reverb, "track 1 sends quietly": quiet,
            "no problems": no_problems(folder)}


def check_untouched(folder: Path) -> dict:
    # A question the app answers by itself: the agent says how, and builds nothing.
    return {"no project tool": not project_tools(folder),
            "pad keeps the synth": instruments(folder).get("pad") == "instrument.synth",
            "no problems": no_problems(folder)}


def check_grid(parts: int, effects: set, patterns: int):
    """One screen whose tool plays `parts` built-in synths through `effects`, from looping
    timelines, and sounds as it starts."""
    def check(folder: Path) -> dict:
        main = window(folder)
        owned = list(instances(folder, main).values()) if main else []
        tools = [found.get("tool") for found in owned]
        loops = [found for found in owned
                 if found.get("tool") == "timeline" and found.get("state", {}).get("loop")]
        return {**check_screen(folder),
                "its parts play the built-in synth": tools.count("instrument.synth") >= parts,
                "through their effects": effects <= set(tools),
                "its patterns loop": len(loops) >= patterns,
                "it sounds": sounds(folder)}
    return check


def check_daw_intact(folder: Path) -> dict:
    return {"no problems": no_problems(folder)}


SCENARIOS = {
    "builtin": (setup_builtin, check_builtin,
                "Put a dotted-eighth echo on the lead, quieter than the dry sound, and make the "
                "pad a bit brighter."),
    "tape-user": (setup_tape_user, check_tape_user,
                  "The worn tape sound on the pad is great. Put that same tape effect on the lead "
                  "too, but like a fairly fresh reel tape so it only wobbles a little. And make "
                  "the pad's tape really worn out, the most it goes."),
    "tape-builder": (setup_tape_builder, check_track("pad"),
                     "I want my pad to sound like it's playing off an old worn tape: that slow, "
                     "seasick pitch wobble (wow) plus a faster little flutter, and a bit of tape "
                     "hiss under it. Give me a knob for how worn the tape is, and let me pick "
                     "between a 'cassette' and a 'reel' character; reel should be gentler and a "
                     "bit brighter. Put it on the pad."),
    "sequencer-builder": (setup_with_bass, check_track("bass"),
                          "On the bass track I want a 16-step acid bass sequencer as its "
                          "instrument: I click steps on and off on its card and the step that "
                          "is playing lights up, it plays along with the tempo of the piece, "
                          "and it has knobs for the filter cutoff, the resonance and the note "
                          "it plays. Make a pattern that grooves to start with."),
    # Many controls: the card must fit 144 points, which the check cannot see. Look at it.
    "drums-builder": (setup_with_bass, check_track("bass"),
                      "On the bass track I want a drum machine as its instrument instead: "
                      "kick, snare, closed hat and clap, each with its own row of 16 steps I "
                      "click on and off, the playing step lit; a level and a tune knob for each "
                      "drum, and a swing knob. Make a groove to start with."),
    # How cards and pages look: short prompts, the screenshots say the rest.
    "ui-scope": (setup_tape_builder, check_track("lead", own_view=True),
                 "Put an effect on the lead that shows its sound as a moving waveform on its "
                 "card."),
    "daw-arp-builder": (setup_daw_chords, check_track("t00"),
                        "On Track 1, put an arpeggiator before its synth: it turns the chords I "
                        "hold into notes going up one at a time with the tempo, with a choice of "
                        "eighths or sixteenths and a knob for how many octaves it climbs."),
    "daw-duplicate": (setup_daw_chords, check_daw_intact,
                      "Let me duplicate a track from its row in the timeline: the copy, with the "
                      "same instrument, effects, mix and clips, lands right under it."),
    "daw-return": (setup_daw, check_daw_return,
                   "Add a return track with a long reverb, and send Track 1 into it, fairly "
                   "quietly."),
    "ui-chords": (setup_tape_builder, check_track("pad", own_view=True),
                  "Make the pad a chord player: I pick a progression on its card, and it shows "
                  "the chord it is on."),
    "ui-tremolo": (setup_tape_builder, check_track("pad", own_view=False),
                   "Put a tremolo on the pad with a rate and a depth knob."),
    "ui-drones": (setup_tape_builder, check_drones,
                  "Turn this project into a drone machine on one screen: four drones, each with "
                  "a pitch and a volume control and a visual of its sound."),
    "ui-piano": (setup_tape_builder, check_track("lead", own_view=True),
                 "Give the lead an instrument with a small piano keyboard on its card that I "
                 "click to play."),
    "ui-bands": (setup_tape_builder, check_track("pad", own_view=True),
                 "Put an effect on the pad that shows the level of 8 frequency bands as bars on "
                 "its card."),
    "ui-paint": (setup_tape_builder, check_screen,
                 "Turn this project into one screen I paint on with the mouse; a line sweeps "
                 "across and plays what I painted, higher up is higher pitch."),
    "keys-builder": (setup_tape_builder, check_screen,
                     "Turn this project into one screen where my computer keyboard plays notes, "
                     "each letter its own note, and every note draws a ripple."),
    "midi-builder": (setup_tape_builder, check_track("lead"),
                     "Make the lead an arpeggiator I play from my MIDI keyboard: I hold a chord "
                     "and it plays its notes up and down."),
    "apps-builder": (setup_tape_builder, check_apps,
                     "Turn this project into one screen that takes the music playing in Spotify "
                     "and stutters it; the mouse sets how fast and how much."),
    "keys-composer": (setup_tape_builder, check_keys_composer,
                      "I want to play the pad from my computer keyboard."),
    # Probes: what an agent does with what the app has no feature for, or one it has.
    "cc-composer": (setup_tape_builder, check_untouched,
                    "I want the knobs of my MIDI controller to move the pad's filter."),
    "zoom-composer": (setup_tape_builder, check_untouched,
                      "I want to send the sound of this project into a Zoom call."),
    "experiment-builder": (setup_tape_builder, check_screen,
                           "Turn this project into an interactive sound toy instead of a song, "
                           "just one screen and no timeline: a 'gravity harp'. I drop balls "
                           "with the mouse onto a few horizontal strings; when a ball hits a "
                           "string it plucks it, each string a different note of a pentatonic "
                           "scale, and the ball bounces off."),
    "generative-builder": (setup_tape_builder, check_track("pad"),
                           "Make the pad play a generative ambient part by itself, forever: it "
                           "slowly wanders between notes of D dorian with a long, washy tail. "
                           "Give its card a way to change how dense and how bright it is, and "
                           "let me see which note it is on."),
    "life-builder": (setup_tape_builder, check_screen,
                     "Turn this project into a generative toy instead of a song, just one "
                     "screen and no timeline: a Game of Life grid. I click cells to bring them "
                     "to life or kill them, the grid steps on by itself, and a cursor sweeps "
                     "across it column by column, playing the living cells of its column as a "
                     "chord: lower rows are lower notes of a pentatonic scale. A soft, "
                     "bell-like sound."),
    "mic-builder": (setup_tape_builder, check_mic,
                    "Turn this project into a live toy for my voice instead of a song, just one "
                    "screen and no timeline: I sing into the microphone and it comes out as a "
                    "shimmering cloud of echoes, and the louder I sing the bigger a glowing "
                    "circle on the screen gets. Dragging on the screen changes how long the "
                    "echoes last."),
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
    "toy-builder": (setup_tape_builder, check_new_track,
                    "Make me a 'storm' on a new track: a windy, noisy drone I play live with an "
                    "XY pad, across is how dark or bright, up is how wild, plus a 'thunder' "
                    "button that sets off a deep rumble. It should run by itself, no notes "
                    "needed."),
    "launcher-builder": (setup_tape_builder, check_grid(3, {"filter", "reverb", "delay"}, 12),
                         "Turn this project into a clip launcher instead of a song: one screen "
                         "with a grid and no timeline. Three lanes, bass, chords and lead, each "
                         "playing the built-in synth through its own effect: the bass through a "
                         "filter, the chords through a reverb, the lead through a delay. Each lane "
                         "has four patterns. I click a pattern to launch it and it starts on the "
                         "next bar; a column of scene buttons launches a whole row at once. Show "
                         "which patterns play and which wait for the bar, and give me play and "
                         "stop. Start me off with patterns in A minor at 120 BPM that groove "
                         "together, and the first scene playing when I press play."),
    "tracker-builder": (setup_tape_builder, check_grid(2, {"filter", "delay"}, 2),
                        "Turn this project into a step sequencer on one screen instead of a song, "
                        "no timeline: a grid of 16 steps across and two octaves of C minor up the "
                        "side, for two parts, a bass on the built-in synth through a filter and a "
                        "lead on the built-in synth through a delay. I click cells to add or "
                        "remove notes while it plays and hear the change on the next pass, and a "
                        "column lights up as it plays. Each part has a length of 1 to 4 bars and "
                        "I can switch which bar of it I'm looking at. Give me play and stop, and "
                        "start me off with a groove."),
    # Follow-ups: the second message goes on in the same conversation, on what the first built.
    "sequencer-followup": (setup_with_bass, check_track("bass"), [
        "On the bass track I want a 16-step acid bass sequencer as its instrument: I click steps "
        "on and off on its card and the step that is playing lights up, it plays along with the "
        "tempo of the piece, and it has knobs for the filter cutoff, the resonance and the note "
        "it plays. Make a pattern that grooves to start with.",
        "Nice. Now give each step a velocity: dragging a step up or down sets how hard it plays, "
        "and the step shows it by its height. And add a button that shuffles the pattern."]),
    "life-followup": (setup_tape_builder, check_screen, [
        "Turn this project into a generative toy instead of a song, just one screen and no "
        "timeline: a Game of Life grid. I click cells to bring them to life or kill them, the "
        "grid steps on by itself, and a cursor sweeps across it column by column, playing the "
        "living cells of its column as a chord: lower rows are lower notes of a pentatonic scale.",
        "Love it. Let me paint living cells by dragging across the grid, and add a control for "
        "how fast it steps and a button that clears the grid."]),
    "daw-peak": (setup_daw_chords, check_daw_intact,
                 "Over each strip of the mixer, show the loudest level of the last few seconds as "
                 "a number in dB; it drops back after about three seconds."),
}


# What the composer asked for, as the QA council checks it by using the result. Hidden from the
# builder. These scenarios are the `ui` set: new tools, changes to the DAW template, and
# follow-ups on what an agent built.
CRITERIA = {
    "sequencer-builder": {
        "steps": "Clicking a step on the bass's card turns it on, clicking it again turns it off, and the record changes with it.",
        "playing step": "While the piece plays, the playing step is lit and moves with the tempo.",
        "knobs": "The card has labelled controls for the filter cutoff, the resonance and the note it plays.",
        "groove": "Before any click, the pattern has steps on and the bass plays notes.",
    },
    "ui-piano": {
        "keyboard": "The lead's card shows a small piano keyboard, white and black keys in the right pattern.",
        "click plays": "Clicking a key plays that note on the lead.",
        "pressed": "A key shows that it is down while it is pressed or sounding.",
        "lead plays": "The lead's clips still play through the new instrument.",
    },
    "life-builder": {
        "one screen": "The window is one screen with a grid of cells and no timeline.",
        "click cells": "Clicking a dead cell brings it to life; clicking a living cell kills it.",
        "steps": "While it plays, the grid steps on by itself by the rules of the Game of Life.",
        "cursor": "A cursor sweeps across the grid column by column while it plays.",
        "chord": "The living cells of the cursor's column sound, lower rows lower notes.",
    },
    "launcher-builder": {
        "grid": "One screen with three lanes (bass, chords, lead) of four patterns each, and no timeline.",
        "launch": "Clicking a pattern shows it waiting, then playing from the next bar.",
        "scenes": "A scene button launches a whole row of patterns at once.",
        "transport": "There is play and stop, and pressing play starts the first scene.",
        "sounds": "The lanes play through a filter, a reverb and a delay, and the patterns groove together in A minor.",
    },
    "daw-duplicate": {
        "way": "Each track's row in the timeline has a visible way to duplicate it: a button or a menu item.",
        "copy": "Using it adds a track right under the original with the same instrument, effects, mix and clips.",
        "name": "The copy is named after the original, such as 'Track 1 copy'.",
        "intact": "The other tracks, their clips and the mixer still work and look as before.",
    },
    "daw-peak": {
        "readout": "While the piece plays, each mixer strip shows the loudest recent level as a number in dB near its meter.",
        "hold": "The number holds the loudest level for a few seconds, then drops back.",
        "intact": "The mixer's faders, pan, mute and solo, and the rest of the song page, still work and look as before.",
    },
    "sequencer-followup": {
        "velocity": "Dragging a step up or down sets how hard it plays; the step shows it by its height, and the record keeps it.",
        "shuffle": "A button shuffles the pattern: the steps change and it still plays.",
        "still works": "Steps still turn on and off with a click, the playing step lights, and the knobs still work.",
    },
    "life-followup": {
        "paint": "Dragging across the grid brings every cell it passes to life.",
        "speed": "A control changes how fast the grid steps on, and the change shows while it plays.",
        "clear": "A button clears the grid.",
        "still works": "Clicking single cells, the stepping and the sweeping cursor still work.",
    },
}

DESIGN = Path(__file__).resolve().parents[2] / "DESIGN.md"
# Each member checks on a copy of its own: a provider and a model.
COUNCIL = {"opus": ("claude", "claude-opus-5-5"), "sol": ("codex", "gpt-6.1-sol")}


def prompts(scenario: str) -> list:
    asked = SCENARIOS[scenario][2]
    return asked if isinstance(asked, list) else [asked]


def last_json(text: str) -> dict:
    for line in reversed(text.strip().splitlines()):
        try:
            return json.loads(line[line.index("{"):])
        except ValueError:
            continue
    return {}


def review(folder: Path, scenario: str, member: str) -> dict:
    """One council member checks each criterion on a copy of the project by using it, and scores
    the UI and the UX."""
    provider, model = COUNCIL[member]
    copy = folder.with_name(f"{folder.name}-{member}")
    shutil.rmtree(copy, ignore_errors=True)
    shutil.copytree(folder, copy, symlinks=True, ignore=shutil.ignore_patterns(".sound-tools.lock"))
    scratch = LOGS / "scratch" / copy.name
    shutil.rmtree(scratch, ignore_errors=True)
    scratch.mkdir(parents=True)
    asked = "\n".join(f"{n + 1}. \"{text}\"" for n, text in enumerate(prompts(scenario)))
    criteria = "\n".join(f"- {name}: {text}" for name, text in CRITERIA[scenario].items())
    prompt = (
        f"An agent just built this in a music project for a composer, who asked, message by "
        f"message:\n{asked}\n\n"
        "Test the result as a strict QA tester and a demanding product designer. Change no code "
        "under extensions/. Check everything by using the project the way the app lets an agent: "
        "`sound-tools --help`, AGENTS.md and agent-docs/, screenshots with clicks and play, "
        f"inspect, analyze and the records under state/. Keep screenshots and scratch files in "
        f"{scratch}, and look at every screenshot you take.\n\n"
        f"Criteria:\n{criteria}\n\n"
        "For each criterion give pass, fail, or unverifiable when you found no way to check it, "
        "with the evidence. Then score from 1 to 5, as a demanding designer:\n"
        f"- ui: how it looks, against the \"Principles\" of {DESIGN}. 5 calm and polished, 3 "
        "works but crowded, misaligned or inconsistent, 1 broken or unreadable.\n"
        "- ux: how it works for a composer, against \"Using the app\" in the same file: can they "
        "find each feature without being told, does each action give clear feedback, do the "
        "gestures follow the app's. 5 obvious and pleasant, 3 works but something is hidden, "
        "surprising or slow, 1 cannot be used.\n\n"
        "End with one line of JSON and nothing after it: {\"criteria\": {\"<name>\": "
        "{\"verdict\": \"pass|fail|unverifiable\", \"evidence\": \"...\"}}, \"ui\": n, "
        "\"ui_why\": \"...\", \"ux\": n, \"ux_why\": \"...\", \"workflow\": [{\"shot\": "
        "\"<absolute path>\", \"caption\": \"what it shows, one line\"}]} with the 3 to 6 "
        "screenshots, in order, that best show how it looks and works."
    )
    log = LOGS / f"{copy.name}.log"
    if provider == "claude":
        run = claude_run.run(copy, prompt, log=log, model=model, tools="Bash,Read,Glob,Grep")
    else:
        run = claude_run.codex(copy, prompt, model, log=log)
    return {"member": member, "model": model, "verdict": last_json(run.result), "cost": run.summary()}


def evaluate(scenario: str, number: int, model: str | None, label: str) -> dict:
    setup, check, _ = SCENARIOS[scenario]
    folder = LOGS / f"{label}-{scenario}-{number}"
    shutil.rmtree(folder, ignore_errors=True)
    folder.mkdir(parents=True)
    # As the app makes a project, and writes its docs while it has it open.
    runtime(folder, "--headless", stdin="quit\n")
    setup(folder)
    # The window makes this folder for every project it opens.
    (folder / "extensions").mkdir(exist_ok=True)
    runtime(folder, "--headless", stdin="quit\n")
    messages, session = [], ""
    for turn, prompt in enumerate(prompts(scenario), start=1):
        run = claude_run.run(folder, prompt, log=LOGS / f"{folder.name}-{turn}.log", model=model,
                             resume=session)
        session = run.session
        messages.append(run.summary())
    checks = check(folder)
    result = {"scenario": scenario, "run": number, "label": label, "model": model,
              "passed": all(checks.values()), "checks": checks, "messages": messages}
    if scenario in CRITERIA:
        with ThreadPoolExecutor() as pool:
            result["council"] = list(pool.map(lambda member: review(folder, scenario, member), COUNCIL))
    return result


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("scenarios", nargs="+", choices=[*SCENARIOS, "ui"])
    parser.add_argument("--runs", type=int, default=1)
    parser.add_argument("--first", type=int, default=1, help="the number of the first run, so "
                        "evals of one scenario can run side by side")
    parser.add_argument("--model", default="claude-opus-5-5")
    parser.add_argument("--label", default="gpui", help="what is being compared, such as the "
                        "SDK: results and folders carry it, and report.py puts labels side by side")
    arguments = parser.parse_args()
    LOGS.mkdir(parents=True, exist_ok=True)
    scenarios = [one for name in arguments.scenarios for one in (CRITERIA if name == "ui" else [name])]
    for scenario in scenarios:
        for number in range(arguments.first, arguments.first + arguments.runs):
            result = evaluate(scenario, number, arguments.model, arguments.label)
            print(json.dumps(result), flush=True)
            with open(LOGS / "results.jsonl", "a") as results:
                results.write(json.dumps(result) + "\n")


if __name__ == "__main__":
    main()
