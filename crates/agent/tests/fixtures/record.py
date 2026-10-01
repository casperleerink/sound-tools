#!/usr/bin/env python3
"""Records the Claude fixtures: real runs of the `claude` CLI, both directions in one file.

Run from anywhere, with a signed-in `claude` on PATH (or CLAUDE=/path/to/claude):

    python3 crates/agent/tests/fixtures/record.py            # every scenario
    python3 crates/agent/tests/fixtures/record.py interrupt  # one of them

Each scenario writes `claude/<name>.jsonl`. Every line is one of
`{"sent": <what we wrote>}`, `{"received": <what the CLI wrote>}` or
`{"exited": {"code": <int or null>, "stderr": <text>}}`. The tests replay the file
through the driver's parser and mapper, so after recording run
`cargo insta test -p sound-agent --review` and read the changed snapshots.

The flags match `provider/claude.rs`. Personal data (account, paths, the user's own
agents) is replaced before writing, since the repository is public. Runs use haiku to
stay cheap; one full recording costs a few cents.
"""

import json
import os
import re
import signal
import subprocess
import sys
import tempfile
import threading
import uuid
from pathlib import Path

CLAUDE = os.environ.get("CLAUDE", "claude")
OUTPUT = Path(__file__).parent / "claude"
TIMEOUT_SECONDS = 180


def environment():
    # The same cleaning as the driver: a nested session never saves its transcript.
    cleaned = {
        key: value
        for key, value in os.environ.items()
        if key != "CLAUDECODE" and not key.startswith("CLAUDE_CODE_") and key != "ELECTRON_RUN_AS_NODE"
    }
    cleaned["DISABLE_AUTOUPDATER"] = "1"
    return cleaned


def arguments(mode, session, model):
    return [
        CLAUDE, "-p",
        "--input-format", "stream-json",
        "--output-format", "stream-json",
        "--verbose",
        "--include-partial-messages",
        "--permission-prompt-tool", "stdio",
        "--permission-mode", mode,
        *session,
        "--model", model,
        "--tools", "Bash,Read,Edit,Write,Glob,Grep",
        "--strict-mcp-config",
        "--setting-sources", "project,local",
        "--disable-slash-commands",
    ]


class Run:
    """One CLI process and the transcript of both directions."""

    def __init__(self, name, mode, session=None, model="haiku"):
        self.name = name
        self.folder = tempfile.mkdtemp(prefix="sound-agent-fixture-")
        session = session or ["--session-id", str(uuid.uuid4())]
        self.process = subprocess.Popen(
            arguments(mode, session, model),
            cwd=self.folder,
            env=environment(),
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            bufsize=1,
        )
        self.lines = []
        self.requests = 0
        self.watchdog = threading.Timer(TIMEOUT_SECONDS, self.process.kill)
        self.watchdog.start()

    def send(self, message):
        try:
            self.process.stdin.write(json.dumps(message) + "\n")
            self.process.stdin.flush()
        except BrokenPipeError:
            # The CLI already exited, as on a stale resume. The file shows what it said.
            return
        self.lines.append({"sent": message})

    def control(self, request):
        self.requests += 1
        self.send({"type": "control_request", "request_id": f"request-{self.requests}", "request": request})

    def user(self, text):
        self.send({
            "type": "user",
            "message": {"role": "user", "content": [{"type": "text", "text": text}]},
            "parent_tool_use_id": None,
            "session_id": "",
        })

    def answer(self, request, response):
        self.send({
            "type": "control_response",
            "response": {"subtype": "success", "request_id": request["request_id"], "response": response},
        })

    def messages(self):
        for line in self.process.stdout:
            message = json.loads(line)
            self.lines.append({"received": message})
            yield message

    def until_result(self, on_message=lambda message: None):
        for message in self.messages():
            on_message(message)
            if message.get("type") == "result":
                return

    def finish(self):
        if self.process.stdin and not self.process.stdin.closed:
            self.process.stdin.close()
        for _ in self.messages():
            pass
        code = self.process.wait()
        self.watchdog.cancel()
        self.lines.append({"exited": {"code": None if code < 0 else code, "stderr": self.process.stderr.read()}})
        self.write()

    def write(self):
        OUTPUT.mkdir(exist_ok=True)
        text = "".join(json.dumps(line) + "\n" for line in self.lines)
        text = scrub(text, self.folder)
        (OUTPUT / f"{self.name}.jsonl").write_text(text)
        print(f"wrote {OUTPUT / (self.name + '.jsonl')}")


def scrub(text, folder):
    home = str(Path.home())
    for real in (os.path.realpath(folder), folder):
        text = text.replace(real, "/tmp/project")
    text = text.replace(home, "/Users/composer")
    text = re.sub(r"[\w.+-]+@[\w-]+\.[\w.]+", "composer@example.com", text)
    lines = []
    for line in text.splitlines():
        entry = json.loads(line)
        received = entry.get("received", {})
        # The user's own agents are listed whatever the setting sources; they are not ours to publish.
        response = received.get("response", {}).get("response")
        if isinstance(response, dict) and "agents" in response:
            response["agents"] = []
        if received.get("subtype") == "init":
            received["agents"] = []
        lines.append(json.dumps(entry))
    return "\n".join(lines) + "\n"


def allow(run, message):
    if message.get("type") == "control_request" and message["request"]["subtype"] == "can_use_tool":
        run.answer(message, {"behavior": "allow", "updatedInput": message["request"]["input"]})


def plain():
    run = Run("plain", "acceptEdits")
    run.control({"subtype": "initialize"})
    run.user("Say hello in one short sentence. Use no tools.")
    run.until_result()
    run.finish()


def edit():
    run = Run("edit", "acceptEdits")
    run.control({"subtype": "initialize"})
    run.user(
        "Create notes.txt containing the line 'one' with the Write tool. Then change 'one' to 'two' "
        "with the Edit tool. Then reply with the single word: done."
    )
    run.until_result(lambda message: allow(run, message))
    run.finish()


def approval(name, response):
    run = Run(name, "default")
    run.control({"subtype": "initialize"})
    run.user("Run exactly this command with the Bash tool: touch marker.txt. Then reply in one short sentence.")

    def on_message(message):
        if message.get("type") == "control_request" and message["request"]["subtype"] == "can_use_tool":
            run.answer(message, response(message))

    run.until_result(on_message)
    run.finish()


def approval_allowed():
    approval(
        "approval_allowed",
        lambda message: {"behavior": "allow", "updatedInput": message["request"]["input"]},
    )


def approval_denied():
    approval(
        "approval_denied",
        lambda message: {"behavior": "deny", "message": "The composer denied this."},
    )


def interrupt():
    run = Run("interrupt", "acceptEdits")
    run.control({"subtype": "initialize"})
    run.user("Write a 400-word story about a cello. Use no tools.")
    deltas = 0
    for message in run.messages():
        event = message.get("event", {})
        if event.get("type") == "content_block_delta" and event.get("delta", {}).get("type") == "text_delta":
            deltas += 1
            if deltas == 5:
                run.control({"subtype": "interrupt"})
        if message.get("type") == "result":
            break
    # The process must take the next message after an interrupt.
    run.user("Say ok. Use no tools.")
    run.until_result()
    run.finish()


def permission_mode():
    run = Run("permission_mode", "default")
    run.control({"subtype": "initialize"})
    run.user("Create a.txt containing the letter a, with the Write tool. Then reply: done.")
    run.until_result(lambda message: allow(run, message))
    # From here the Write must not ask. The recorder still allows one if it does, so the
    # fixture shows it.
    run.control({"subtype": "set_permission_mode", "mode": "acceptEdits"})
    run.user("Create b.txt containing the letter b, with the Write tool. Then reply: done.")
    run.until_result(lambda message: allow(run, message))
    run.finish()


def stale_resume():
    run = Run("stale_resume", "acceptEdits", ["--resume", str(uuid.uuid4())])
    run.control({"subtype": "initialize"})
    run.user("Say ok.")
    run.finish()


def error_turn():
    # A model that does not exist: the CLI starts, and the turn ends with an error.
    run = Run("error_turn", "acceptEdits", model="claude-no-such-model")
    run.control({"subtype": "initialize"})
    run.user("Say ok.")
    run.until_result()
    run.finish()


def crash():
    run = Run("crash", "acceptEdits")
    run.control({"subtype": "initialize"})
    run.user("Write a 400-word story about a cello. Use no tools.")
    for message in run.messages():
        event = message.get("event", {})
        if event.get("type") == "content_block_delta" and event.get("delta", {}).get("type") == "text_delta":
            run.process.send_signal(signal.SIGKILL)
            break
    run.finish()


SCENARIOS = {
    function.__name__: function
    for function in (plain, edit, approval_allowed, approval_denied, interrupt, permission_mode, stale_resume, error_turn, crash)
}

if __name__ == "__main__":
    for name in sys.argv[1:] or SCENARIOS:
        SCENARIOS[name]()
