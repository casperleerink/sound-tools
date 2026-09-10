"""Run cached extension edit/build/restart cycles with first-frame readiness."""
import argparse
import json
from pathlib import Path
import re
import selectors
import subprocess
import time

ROOT = Path(__file__).resolve().parent
RESULTS = ROOT / 'results'
SOURCE = ROOT / 'extension/src/lib.rs'
BINARY = ROOT / 'target/debug/sound-tools-timing'
COMMAND = ['cargo', 'rustc', '--offline', '--locked', '-p', 'sound-tools-timing',
           '--message-format=json', '--', '-C', f'linker={ROOT / "linker.py"}']

def build(name):
    linker = RESULTS / 'linker.jsonl'
    before = len(linker.read_text().splitlines()) if linker.exists() else 0
    start = time.perf_counter()
    with (RESULTS / f'{name}.jsonl').open('w') as out, (RESULTS / f'{name}.log').open('w') as err:
        completed = subprocess.run(COMMAND, cwd=ROOT, stdout=out, stderr=err)
    elapsed = time.perf_counter() - start
    messages = [json.loads(line) for line in (RESULTS / f'{name}.jsonl').read_text().splitlines()]
    rebuilt = [m['target']['name'] for m in messages if m.get('reason') == 'compiler-artifact' and not m['fresh']]
    links = [json.loads(line) for line in linker.read_text().splitlines()[before:]] if linker.exists() else []
    return {'seconds': elapsed, 'exit_code': completed.returncode, 'rebuilt': rebuilt, 'links': links}

def launch(name):
    start = time.perf_counter()
    log = (RESULTS / f'{name}-runtime.log').open('w')
    process = subprocess.Popen([str(BINARY)], cwd=ROOT, stdout=subprocess.PIPE, stderr=log, text=True, bufsize=1)
    selector = selectors.DefaultSelector()
    selector.register(process.stdout, selectors.EVENT_READ)
    lines = []
    try:
        while time.perf_counter() - start < 30:
            if selector.select(timeout=0.1):
                line = process.stdout.readline()
                lines.append(line.strip())
                if line.startswith('FIRST_FRAME '):
                    return process, {'seconds': time.perf_counter() - start, 'marker': line.strip(), 'pid': process.pid}
            if process.poll() is not None:
                raise RuntimeError(f'Runtime exited {process.returncode}: {lines}')
        raise RuntimeError(f'No first frame within 30 seconds: {lines}')
    except BaseException:
        process.terminate()
        process.wait(timeout=10)
        raise
    finally:
        selector.close()
        log.close()

def stop(process):
    process.terminate()
    process.wait(timeout=10)

if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--runs', type=int, default=7)
    args = parser.parse_args()
    initial_source = SOURCE.read_text()
    results = []
    process = None
    try:
        process, initial = launch('initial')
        for index in range(1, args.runs + 1):
            started = time.perf_counter()
            revision = f'revision-{index}'
            SOURCE.write_text(re.sub(r'pub const REVISION: &str = "[^"]+";', f'pub const REVISION: &str = "{revision}";', initial_source))
            built = build(f'edit-{index}')
            assert built['exit_code'] == 0, built
            assert process.poll() is None, 'Previous runtime died during build'
            restart_started = time.perf_counter()
            stop(process)
            process, launched = launch(f'edit-{index}')
            restart = time.perf_counter() - restart_started
            assert revision in launched['marker'], launched
            row = {'run': index, 'build': built, 'launch': launched, 'stop_to_frame_seconds': restart,
                   'edit_to_frame_seconds': time.perf_counter() - started, 'previous_runtime_survived_build': True}
            results.append(row)
            (RESULTS / 'measurements.json').write_text(json.dumps(results, indent=2))
            print(json.dumps(row), flush=True)
    finally:
        if process is not None and process.poll() is None:
            stop(process)
        SOURCE.write_text(initial_source)
