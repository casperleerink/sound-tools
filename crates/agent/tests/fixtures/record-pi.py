#!/usr/bin/env python3
"""Records the Pi fixtures: real runs of `pi --mode rpc` with our extension, both directions
in one file, in the format of `record.py`.

Run with a signed-in `pi` on PATH that has an OpenRouter key (or PI=/path/to/pi):

    python3 crates/agent/tests/fixtures/record-pi.py            # every scenario
    python3 crates/agent/tests/fixtures/record-pi.py interrupt  # one of them

Each scenario writes `crates/agent/tests/fixtures/pi/<name>.jsonl`; then run
`cargo insta test -p sound-agent --review`. The flags match
`crates/agent/src/provider/pi/mod.rs`. The models are cut to the run's own, and the folder is
written as /tmp/project, and the system prompt is left out, since the repository is public.
"""

import json
import os
import subprocess
import sys
import tempfile
import uuid
from pathlib import Path

HERE = Path(__file__).resolve().parent
EXTENSION = HERE.parent.parent / "src/provider/pi/extension.js"
MODEL = "openrouter/google/gemini-2.5-flash-lite"

# What each scenario sends, and how it answers the approval it is asked.
SCENARIOS = {
    "plain": ("ask_before_commands", "Reply with the two words: hello there.", None),
    "approval_allowed": (
        "ask_before_commands",
        "Use the bash tool to run: echo hi > b.txt. Then reply done.",
        "allow",
    ),
    "approval_denied": (
        "ask_for_everything",
        "Use the write tool to create c.txt containing yo. Then reply done.",
        "deny",
    ),
    "interrupt": (
        "ask_before_commands",
        "Use the bash tool to run: echo again > d.txt. Then reply done.",
        "abort",
    ),
}


def record(name, mode, message, answer):
    folder = Path(tempfile.mkdtemp())
    (folder / "a.txt").write_text("hello\n")
    program = os.environ.get("PI", "pi")
    arguments = [
        program, "--no-extensions", "--extension", str(EXTENSION), "--no-skills",
        "--no-prompt-templates", "--no-themes", "--no-mcp", "--no-approve", "--mode", "rpc",
        "--session-id", str(uuid.uuid4()), "--tools", "read,bash,edit,write,grep,find,ls",
        "--sound-tools-approval", mode, "--model", MODEL,
    ]
    process = subprocess.Popen(
        arguments, cwd=folder, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    lines = []

    def send(record):
        lines.append({"sent": record})
        process.stdin.write((json.dumps(record) + "\n").encode())
        process.stdin.flush()

    send({"type": "prompt", "id": "models-1", "message": "/sound-tools-models"})
    send({"type": "prompt", "id": "message-2", "message": message})
    for line in process.stdout:
        record = json.loads(line)
        kind = record.get("type")
        # The system prompt holds the recorder's own Pi instructions, and the driver reads none.
        message = record.get("message")
        if isinstance(message, dict) and message.get("role") == "system":
            continue
        if "messages" in record:
            record["messages"] = []
        if kind == "extension_ui_request" and record.get("method") == "notify":
            said = json.loads(record["message"])
            if "models" in said:
                said["models"] = [m for m in said["models"] if m["id"] == MODEL]
                record["message"] = json.dumps(said)
        lines.append({"received": record})
        if kind == "extension_ui_request" and record.get("method") == "select":
            if answer == "abort":
                send({"type": "abort", "id": "stop-3"})
            else:
                send({"type": "extension_ui_response", "id": record["id"], "value": answer})
        if kind == "agent_settled":
            break
    process.stdin.close()
    stderr = process.stderr.read().decode()
    code = process.wait()
    stderr = "\n".join(line for line in stderr.splitlines() if not line.startswith("Warning:"))
    lines.append({"exited": {"code": code, "stderr": stderr}})
    text = "\n".join(json.dumps(line) for line in lines) + "\n"
    text = text.replace(str(folder), "/tmp/project").replace(str(folder.resolve()), "/tmp/project")
    out = HERE / "pi" / f"{name}.jsonl"
    out.parent.mkdir(exist_ok=True)
    out.write_text(text)
    print(f"wrote {out}")


for name in sys.argv[1:] or SCENARIOS:
    record(name, *SCENARIOS[name])
