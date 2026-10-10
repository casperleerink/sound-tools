"""Measures the runtime of git commits: offline render CPU, live playback, null tests. See README.md."""

import argparse
import json
import math
import os
import platform
import re
import shutil
import statistics
import struct
import subprocess
import sys
import threading
import time
from pathlib import Path

import projects

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
ROOT = Path(os.environ.get("PERF_DIR", "/tmp/st-perf"))
# A temporary HOME for every run, so no setting or plugin cache of this machine plays a part.
# Bun is found on the PATH then, not in ~/.bun.
ENV = dict(os.environ, HOME=str(ROOT / "home"), SOUND_TOOLS_NO_UPDATES="1",
           PATH=os.pathsep.join([str(Path.home() / ".bun/bin"), os.environ.get("PATH", "")]))
# ru_maxrss is bytes on macOS and kilobytes on Linux.
MAXRSS_BYTES = 1 if platform.system() == "Darwin" else 1024
# A `+edit` live run changes the `root` choice of the gravity-harp track every few seconds, so
# its sound is built again while the piece plays. A save of a tool file would do the same, but
# only the window reloads tool files; headless does not.
EDIT_EVERY = 5
ROOTS = ["C", "E", "F", "G", "A", "D"]
# Tools that are silent without a window: their sound only starts from their page.
SILENT = {"one-gravity-harp": "plucks only when a ball drops on its page"}


def git(*arguments, cwd=REPO):
    return subprocess.run(["git", *arguments], cwd=cwd, check=True, capture_output=True, text=True).stdout.strip()


class Build:
    """A release build of one commit, in its own worktree under ROOT/<sha>."""

    def __init__(self, ref):
        self.ref = ref
        self.sha = git("rev-parse", "--short=12", f"{ref}^{{commit}}")
        self.base = ROOT / self.sha
        self.source = self.base / "src"
        self.runtime = self.base / "runtime"
        if self.runtime.exists():
            return
        if not self.source.exists():
            git("worktree", "add", "--detach", str(self.source), self.sha)
        print(f"building {ref} ({self.sha})", file=sys.stderr, flush=True)
        # One target folder for all commits: dependencies build once, each commit links its own.
        target = ROOT / "target"
        subprocess.run(["cargo", "build", "--release", "-p", "runtime", "--locked"], cwd=self.source, check=True,
                       env=dict(os.environ, CARGO_TARGET_DIR=str(target)))
        shutil.copy(target / "release/runtime", self.runtime)

    def __str__(self):
        return f"{self.ref} ({self.sha[:7]})"

    def project(self, name):
        return self.base / "projects" / name

    def make_projects(self, names):
        for name in dict.fromkeys(name.removesuffix("+edit") for name in names):
            projects.make(self.runtime, self.source, self.project(name), name, ENV)


def check_output(text, what):
    """Fails on any problem or error the runtime printed: a tool that did not load is one."""
    bad = [line for line in text.splitlines()
           if line.startswith("error") or (line.startswith("problems:") and line != "problems: 0")]
    if bad:
        raise SystemExit(f"{what}: the runtime reported problems:\n" + "\n".join(bad))


def render_once(build, name, seconds, wav):
    """Renders `seconds` of a project. Returns cpu and wall seconds, peak RSS in MB, output peak."""
    log = ROOT / "logs" / f"{build.sha}-{name}-{seconds}.txt"
    log.parent.mkdir(parents=True, exist_ok=True)
    started = time.perf_counter()
    with log.open("w") as out:
        process = subprocess.Popen([build.runtime, build.project(name), "--render", wav, "--seconds", str(seconds)],
                                   env=ENV, stdout=out, stderr=subprocess.STDOUT)
        _, status, usage = os.wait4(process.pid, 0)
    wall = time.perf_counter() - started
    text = log.read_text()
    if status != 0:
        raise SystemExit(f"render of {name} on {build} failed:\n{text}")
    check_output(text, f"render of {name} on {build}")
    peak = float(re.search(r"peak ([0-9.]+)", text).group(1))
    return {"cpu": usage.ru_utime + usage.ru_stime, "wall": wall, "mb": usage.ru_maxrss * MAXRSS_BYTES / 2**20,
            "peak": peak}


def render(builds, names, runs, seconds):
    results = {}
    for run in range(runs):
        print(f"render run {run + 1} of {runs}", file=sys.stderr, flush=True)
        for name in names:  # interleaved, so a slow minute of the machine hits every row alike
            for build in builds:
                for length in (1, seconds):
                    results.setdefault((name, build.sha, length), []).append(
                        render_once(build, name, length, ROOT / "out.wav"))
    save("render", {"|".join(map(str, key)): value for key, value in results.items()})

    def median(name, build, length, field):
        return statistics.median(row[field] for row in results[(name, build.sha, length)])

    def dsp(name, build):  # share of one core while it plays, without startup
        return (median(name, build, seconds, "cpu") - median(name, build, 1, "cpu")) / (seconds - 1) * 100

    print(f"\nRender, median of {runs}. dsp: CPU of {seconds} s minus CPU of 1 s, as % of one core.\n")
    print("| project | commit | dsp % of a core | startup cpu s | startup wall s | peak MB |")
    print("| --- | --- | ---: | ---: | ---: | ---: |")
    for name in names:
        for build in builds:
            print(f"| {name} | {build} | {dsp(name, build):.2f} | {median(name, build, 1, 'cpu'):.2f} "
                  f"| {median(name, build, 1, 'wall'):.2f} | {median(name, build, seconds, 'mb'):.0f} |")
    ratios = [(rebuild, builtin, pattern) for rebuild, builtin in projects.REBUILDS.items()
              for pattern in projects.PATTERNS if f"{rebuild}-{pattern}" in names and f"{builtin}-{pattern}" in names]
    if ratios:
        print("\n| rebuild / built-in | " + " | ".join(map(str, builds)) + " |")
        print("| --- |" + " ---: |" * len(builds))
        for rebuild, builtin, pattern in ratios:
            cells = [f"{dsp(f'{rebuild}-{pattern}', b) / dsp(f'{builtin}-{pattern}', b):.1f}x" for b in builds]
            print(f"| {rebuild} / {builtin}, {pattern} | " + " | ".join(cells) + " |")


def process_tree(root):
    """pid -> (cpu seconds, rss bytes, command) of `root` and everything it started."""
    rows = subprocess.run(["ps", "-A", "-o", "pid=,ppid=,time=,rss=,comm="], capture_output=True, text=True).stdout
    processes = {}
    for line in rows.splitlines():
        pid, parent, cpu, rss, command = line.split(None, 4)
        seconds = 0.0
        for part in cpu.replace("-", ":").split(":"):
            seconds = seconds * 60 + float(part)
        processes[int(pid)] = (int(parent), seconds, int(rss) * 1024, command)
    found, todo = {}, [root]
    while todo:
        pid = todo.pop()
        if pid in processes:
            found[pid] = processes[pid][1:]
            todo += [child for child, row in processes.items() if row[0] == pid]
    return found


def edit_choice(folder, edits):
    record = {"tool": "gravity-harp", "state": {"root": ROOTS[edits % len(ROOTS)]}}
    (folder / "state/arrangement/gravity-harp/instrument.json").write_text(json.dumps(record))


def live_once(build, job, window):
    """Plays a copy of a project headless at -60 dB for `window` seconds."""
    name = job.removesuffix("+edit")
    folder = build.base / "live" / name
    shutil.rmtree(folder, ignore_errors=True)
    shutil.copytree(build.project(name), folder)
    projects.set_master_gain(folder, -60.0)
    process = subprocess.Popen([build.runtime, folder, "--headless"], env=ENV, text=True, bufsize=1,
                               stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    lines = []

    def read():
        for line in process.stdout:
            lines.append(line)

    reader = threading.Thread(target=read, daemon=True)
    reader.start()
    deadline = time.monotonic() + 60
    while "ready\n" not in lines:
        if process.poll() is not None or time.monotonic() > deadline:
            raise SystemExit(f"{job} on {build} did not start:\n{''.join(lines)}")
        time.sleep(0.1)
    process.stdin.write("play\n")
    time.sleep(2)  # past the first blocks of playback
    before, started = process_tree(process.pid), time.monotonic()
    rss, bun_rss, edits = [], [], 0
    while (elapsed := time.monotonic() - started) < window:
        time.sleep(1)
        tree = process_tree(process.pid)
        rss.append(sum(row[1] for row in tree.values()))
        bun_rss.append(sum(row[1] for row in tree.values() if "bun" in row[2]))
        if job.endswith("+edit") and elapsed // EDIT_EVERY > edits:
            edit_choice(folder, edits)
            edits += 1
    after, elapsed = process_tree(process.pid), time.monotonic() - started
    process.stdin.write("status\nquit\n")
    process.stdin.flush()
    process.wait(timeout=60)
    reader.join(timeout=5)
    text = "".join(lines)
    (ROOT / "logs").mkdir(exist_ok=True)
    (ROOT / "logs" / f"{build.sha}-live-{job}.txt").write_text(text)
    check_output(text, f"live {job} on {build}")

    def cpu(tree, only_bun=False):
        return sum(row[0] for row in tree.values() if not only_bun or "bun" in row[2])

    def count(label):
        return re.search(rf"^{label}: (.+)$", text, re.MULTILINE).group(1)

    return {"cpu": (cpu(after) - cpu(before)) / elapsed * 100,
            "bun cpu": (cpu(after, True) - cpu(before, True)) / elapsed * 100,
            "mb": max(rss) / 2**20, "bun mb": max(bun_rss) / 2**20, "edits": edits,
            "engine edits": int(re.findall(r"(\d+) edits applied", text)[-1]),
            "callbacks": int(count("callbacks")), "late": int(count("late callbacks")),
            "slowest": count("slowest callback"), "xruns": int(count("xruns"))}


def live(builds, jobs, runs, window):
    results = {}
    for run in range(runs):
        print(f"live run {run + 1} of {runs}", file=sys.stderr, flush=True)
        for job in jobs:
            for build in builds:
                results.setdefault((job, build.sha), []).append(live_once(build, job, window))
    save("live", {"|".join(key): value for key, value in results.items()})
    print(f"\nLive, {runs} runs of {window} s, master at -60 dB. CPU and MB: median, process tree "
          "(runtime and Bun). Late, xruns and slowest callback: each run, over the whole session.\n")
    print("| project | commit | cpu % | bun cpu % | MB | bun MB | late callbacks | xruns | slowest callback "
          "| engine edits |")
    print("| --- | --- | ---: | ---: | ---: | ---: | --- | --- | --- | --- |")
    for job in jobs:
        for build in builds:
            rows = results[(job, build.sha)]

            def median(field, rows=rows):
                return statistics.median(row[field] for row in rows)

            def each(field, rows=rows):
                return ", ".join(str(row[field]) for row in rows)

            print(f"| {job} | {build} | {median('cpu'):.1f} | {median('bun cpu'):.1f} | {median('mb'):.0f} "
                  f"| {median('bun mb'):.0f} | {each('late')} | {each('xruns')} | {each('slowest')} "
                  f"| {each('engine edits')} |")


def read_wav(path):
    """The float samples of a WAV the runtime wrote (32-bit float, any channels)."""
    data = path.read_bytes()
    at = 12
    while at < len(data):
        chunk, size = data[at:at + 4], struct.unpack_from("<I", data, at + 4)[0]
        if chunk == b"data":
            body = data[at + 8:at + 8 + size]
            return struct.unpack(f"<{len(body) // 4}f", body)
        at += 8 + size + size % 2
    raise SystemExit(f"{path} has no data")


def null(builds, names, seconds):
    if len(builds) != 2:
        raise SystemExit("null takes two commits")
    rows, failed = [], False
    for name in names:
        renders = []
        for index, build in enumerate(builds):
            wav = ROOT / "null" / f"{name}-{index}.wav"
            wav.parent.mkdir(exist_ok=True)
            peak = render_once(build, name, seconds, wav)["peak"]
            if peak < 1e-4 and name not in SILENT:
                raise SystemExit(f"{name} on {build} rendered silence (peak {peak})")
            renders.append(read_wav(wav))
        first, second = renders
        if len(first) != len(second):
            raise SystemExit(f"{name}: the renders differ in length")
        difference = max((abs(a - b) for a, b in zip(first, second)), default=0.0)
        db = 20 * math.log10(difference) if difference > 0 else float("-inf")
        failed |= db > -100
        note = f" (silent: {SILENT[name]})" if name in SILENT else ""
        rows.append(f"| {name} | {max(map(abs, first)):.3f} | {db:.1f}{note} |")
    print(f"\nNull test, {seconds} s, {builds[0]} against {builds[1]}. Target: under -100 dB.\n")
    print("| project | peak | peak difference dB |\n| --- | ---: | ---: |")
    print("\n".join(rows))
    if failed:
        raise SystemExit("null test failed: a difference is above -100 dB")


def profile(build, name, seconds):
    """Samples a render with macOS `sample` and prints the functions most often on top of the stack."""
    # Longer than the sampling takes at any speed; it is stopped when the sampling ends.
    process = subprocess.Popen([build.runtime, build.project(name), "--render", ROOT / "out.wav", "--seconds", "3600"],
                               env=ENV, stdout=subprocess.DEVNULL)
    time.sleep(2)  # past the startup
    report = ROOT / f"profile-{build.sha}-{name}.txt"
    subprocess.run(["sample", str(process.pid), str(seconds), "-file", report], check=True, capture_output=True)
    process.kill()
    process.wait()
    section = report.read_text().split("Sort by top of stack")[1].split("Binary Images")[0]
    section = subprocess.run(["c++filt"], input=section, capture_output=True, text=True).stdout or section
    # Kernel calls are threads waiting, not work.
    top = [line.rsplit(None, 1) for line in section.splitlines()[1:] if line.strip() and "libsystem_kernel" not in line]
    total = sum(int(count) for _, count in top)
    print(f"\nTop of stack, {name} on {build}: share of busy samples over {seconds} s, all threads. "
          f"Full report: {report}\n")
    for function, count in top[:15]:
        print(f"{int(count) * 100 / total:5.1f}%  {function.strip()}")


def save(kind, results):
    folder = ROOT / "results"
    folder.mkdir(exist_ok=True)
    (folder / f"{kind}-{time.strftime('%Y%m%d-%H%M%S')}.json").write_text(json.dumps(results, indent=1))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=["render", "live", "null", "profile"])
    parser.add_argument("refs", nargs="+", help="commits or branches, such as main HEAD")
    parser.add_argument("--projects", help="comma separated; see projects.names()")
    parser.add_argument("--runs", type=int, default=3)
    parser.add_argument("--seconds", type=int, help="render length (render 61, null 20, profile 10)")
    parser.add_argument("--window", type=int, default=30, help="seconds of live playback measured")
    arguments = parser.parse_args()
    defaults = {
        "render": projects.names(),
        "live": ["builtin12", "mixed", "mixed-x2", "mixed+edit"],
        "null": [name for name in projects.names() if name.startswith(("one-", *projects.REBUILDS))],
        "profile": ["one-grain-cloud"],
    }
    names = arguments.projects.split(",") if arguments.projects else defaults[arguments.mode]
    (ROOT / "home").mkdir(parents=True, exist_ok=True)
    builds = [Build(ref) for ref in arguments.refs]
    for build in builds:
        build.make_projects(names)
    if arguments.mode == "render":
        render(builds, names, arguments.runs, arguments.seconds or 61)
    elif arguments.mode == "live":
        live(builds, names, arguments.runs, arguments.window)
    elif arguments.mode == "null":
        null(builds, names, arguments.seconds or 20)
    else:
        for build in builds:
            for name in names:
                profile(build, name, arguments.seconds or 10)


if __name__ == "__main__":
    main()
