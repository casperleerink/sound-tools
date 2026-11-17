"""Puts eval runs side by side as one HTML page: per scenario a column per `--label`, with what
the builder spent, each council member's verdicts, UI and UX scores and the screenshots it kept.

    python3 tooling/agent-eval/report.py [--labels before,after] [--out ~/.cache/sound-tools-eval/report]
"""

import argparse
import html
import json
import shutil
import statistics
from pathlib import Path

from eval import CRITERIA, LOGS, prompts

COLOURS = {"pass": "#7ee0a0", "fail": "#ff6b6b", "unverifiable": "#f0b44c"}
STYLE = """
body { background:#16171a; color:#e6e7ea; font:14px/1.45 -apple-system, sans-serif; margin:32px; }
h1, h2 { font-weight:600; } h2 { margin-top:48px; border-top:1px solid #2c2f35; padding-top:24px; }
.asked { color:#8b8f98; max-width:960px; } table { border-collapse:collapse; }
td, th { padding:6px 12px; text-align:left; vertical-align:top; border-bottom:1px solid #2c2f35; }
.cols { display:grid; grid-auto-flow:column; grid-auto-columns:minmax(420px, 1fr); gap:24px; }
.col { background:#1e2024; border-radius:8px; padding:16px; }
.member { margin-top:16px; } .muted { color:#8b8f98; font-size:12px; }
.dot { display:inline-block; width:8px; height:8px; border-radius:4px; margin-right:6px; }
figure { margin:8px 0; } img { width:100%; border-radius:6px; border:1px solid #2c2f35; }
figcaption { color:#8b8f98; font-size:12px; }
"""


def latest(results: list) -> dict:
    """The last run of each scenario, run number and label."""
    found = {}
    for result in results:
        if "council" in result:
            found[(result["scenario"], result["label"], result["run"])] = result
    return found


def picture(path: str, out: Path) -> str:
    source = Path(path)
    if not source.is_file():
        return ""
    name = "-".join(source.parts[-2:])
    (out / "img").mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source, out / "img" / name)
    return f"img/{html.escape(name)}"


def member(review: dict, out: Path) -> str:
    verdict = review.get("verdict", {})
    cost = review.get("cost", {})
    rows = "".join(
        f'<div title="{html.escape(v.get("evidence", ""))}"><span class="dot" style="background:'
        f'{COLOURS.get(v.get("verdict"), "#555")}"></span>{html.escape(name)}: {v.get("verdict")}</div>'
        for name, v in verdict.get("criteria", {}).items())
    shots = "".join(
        f'<figure><img src="{src}"><figcaption>{html.escape(step.get("caption", ""))}</figcaption></figure>'
        for step in verdict.get("workflow", []) if (src := picture(step.get("shot", ""), out)))
    return (f'<div class="member"><b>{html.escape(review["member"])}</b> '
            f'<span class="muted">{cost.get("seconds")} s, {cost.get("tool_calls")} calls</span>'
            f'<div>UI {verdict.get("ui", "?")} · UX {verdict.get("ux", "?")}</div>'
            f'<div class="muted">UI: {html.escape(verdict.get("ui_why", ""))}<br>'
            f'UX: {html.escape(verdict.get("ux_why", ""))}</div>{rows}{shots}</div>')


def column(result: dict, out: Path) -> str:
    messages = "".join(
        f'<div class="muted">message {n}: {m["seconds"]} s, {m["tool_calls"]} calls, '
        f'${m["cost_usd"]}, checked {html.escape(json.dumps(m["checked"]))}</div>'
        for n, m in enumerate(result["messages"], start=1))
    checks = " ".join(f'{name} {"✓" if ok else "✗"}' for name, ok in result["checks"].items())
    council = "".join(member(review, out) for review in result["council"])
    return (f'<div class="col"><b>{html.escape(result["label"])}</b> run {result["run"]}'
            f'{messages}<div class="muted">{html.escape(checks)}</div>{council}</div>')


def summary(results: list) -> str:
    rows = ""
    for label in dict.fromkeys(r["label"] for r in results):
        mine = [r for r in results if r["label"] == label]
        reviews = [review.get("verdict", {}) for r in mine for review in r["council"]]
        verdicts = [v.get("verdict") for review in reviews for v in review.get("criteria", {}).values()]
        mean = lambda key: round(statistics.mean(x[key] for x in reviews if isinstance(x.get(key), (int, float))), 1) \
            if any(isinstance(x.get(key), (int, float)) for x in reviews) else "?"
        messages = [m for r in mine for m in r["messages"]]
        rows += (f"<tr><td>{html.escape(label)}</td><td>{len(mine)}</td>"
                 + "".join(f"<td>{verdicts.count(v)}</td>" for v in COLOURS)
                 + f"<td>{mean('ui')}</td><td>{mean('ux')}</td>"
                 f"<td>{sum(m['seconds'] for m in messages)} s</td>"
                 f"<td>${round(sum(m['cost_usd'] for m in messages), 2)}</td></tr>")
    return ("<table><tr><th>label</th><th>runs</th><th>pass</th><th>fail</th><th>unverifiable</th>"
            f"<th>UI</th><th>UX</th><th>build time</th><th>build cost</th></tr>{rows}</table>")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--results", type=Path, default=LOGS / "results.jsonl")
    parser.add_argument("--labels", help="which labels, in this order; all by default")
    parser.add_argument("--out", type=Path, default=LOGS / "report")
    arguments = parser.parse_args()
    found = latest(json.loads(line) for line in arguments.results.read_text().splitlines() if line.strip())
    labels = arguments.labels.split(",") if arguments.labels else list(dict.fromkeys(k[1] for k in found))
    results = [r for k, r in sorted(found.items()) if k[1] in labels]
    shutil.rmtree(arguments.out, ignore_errors=True)
    arguments.out.mkdir(parents=True)
    body = f"<h1>Agent eval</h1>{summary(results)}"
    for scenario in CRITERIA:
        mine = sorted((r for r in results if r["scenario"] == scenario),
                      key=lambda r: (labels.index(r["label"]), r["run"]))
        if not mine:
            continue
        asked = "".join(f"<p>“{html.escape(p)}”</p>" for p in prompts(scenario))
        body += (f'<h2>{scenario}</h2><div class="asked">{asked}</div>'
                 f'<div class="cols">{"".join(column(r, arguments.out) for r in mine)}</div>')
    (arguments.out / "index.html").write_text(
        f"<!doctype html><meta charset=utf-8><style>{STYLE}</style>{body}")
    print(arguments.out / "index.html")


if __name__ == "__main__":
    main()
