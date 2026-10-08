"""Runs Claude Code the way the app's agent runs it, in a project folder, and measures the run.

The flags are the app's (`crates/agent/src/provider/claude/mod.rs`): the same tools, no setting
sources, no MCP, no slash commands, and the project as an added folder so its CLAUDE.md loads.
`sound-tools` on the PATH is this checkout's debug build.
"""

import json
import os
import subprocess
import time
from dataclasses import dataclass, field
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
RUNTIME = REPO / "target" / "debug" / "runtime"
BIN = Path("/tmp/sound-tools-eval-bin")
APP_TOOLS = "Bash,Read,Edit,Write,Glob,Grep"


def sound_tools_on_path() -> str:
    """A folder with `sound-tools` in it, as the app links it, for the agent's PATH."""
    BIN.mkdir(parents=True, exist_ok=True)
    program = BIN / "sound-tools"
    program.write_text(f'#!/bin/sh\nexec "{RUNTIME}" "$@"\n')
    program.chmod(0o755)
    return f"{BIN}:{os.environ['PATH']}"


@dataclass
class Run:
    result: str
    turns: int
    seconds: float
    cost_usd: float
    input_tokens: int
    output_tokens: int
    is_error: bool
    # Every tool call as (tool, short input), in order.
    calls: list = field(default_factory=list)

    def files_read(self) -> list:
        return [path for tool, path in self.calls if tool == "Read"]

    def skills_used(self) -> list:
        return [name for tool, name in self.calls if tool == "Skill"]

    def summary(self) -> dict:
        return {
            "turns": self.turns,
            "seconds": round(self.seconds),
            "cost_usd": round(self.cost_usd, 3),
            "input_tokens": self.input_tokens,
            "output_tokens": self.output_tokens,
            "error": self.is_error,
            "read": self.files_read(),
            "skills": self.skills_used(),
        }


def short(tool: str, arguments: dict) -> str:
    for key in ("file_path", "skill", "command", "pattern", "path"):
        if key in arguments:
            return str(arguments[key])[:160]
    return json.dumps(arguments)[:160]


def run(folder: Path, prompt: str, tools: str = APP_TOOLS, setting_sources: str = "",
        timeout: int = 1800, log: Path | None = None, slash_commands: bool = False) -> Run:
    """One headless run of Claude Code in `folder`, with the app's flags."""
    environment = {
        key: value
        for key, value in os.environ.items()
        if key != "CLAUDECODE" and not key.startswith("CLAUDE_CODE_")
    }
    environment["PATH"] = sound_tools_on_path()
    arguments = [
        "claude", "-p", prompt,
        "--output-format", "stream-json", "--verbose",
        "--permission-mode", "bypassPermissions",
        "--tools", tools,
        "--strict-mcp-config",
        "--setting-sources", setting_sources,
        "--add-dir", str(folder),
    ]
    if not slash_commands:
        # The app's flag. It also turns skills off.
        arguments.append("--disable-slash-commands")
    started = time.monotonic()
    process = subprocess.run(arguments, cwd=folder, env=environment, capture_output=True,
                             text=True, timeout=timeout)
    seconds = time.monotonic() - started
    if log:
        log.write_text(process.stdout + "\n--- stderr ---\n" + process.stderr)
    calls, final = [], {}
    for line in process.stdout.splitlines():
        try:
            message = json.loads(line)
        except json.JSONDecodeError:
            continue
        if message.get("type") == "assistant":
            for block in message.get("message", {}).get("content", []):
                if block.get("type") == "tool_use":
                    calls.append((block["name"], short(block["name"], block.get("input", {}))))
        if message.get("type") == "result":
            final = message
    usage = final.get("usage", {})
    return Run(
        result=final.get("result", process.stderr[-2000:]),
        turns=final.get("num_turns", 0),
        seconds=seconds,
        cost_usd=final.get("total_cost_usd", 0.0),
        input_tokens=usage.get("input_tokens", 0) + usage.get("cache_read_input_tokens", 0)
        + usage.get("cache_creation_input_tokens", 0),
        output_tokens=usage.get("output_tokens", 0),
        is_error=final.get("is_error", True),
        calls=calls,
    )
