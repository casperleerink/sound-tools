"""Verify successful/failed DSP code reload against one persisted project.

Run with desktop access. Only temporary project data and processes we launch are used.
Source edits are restored in finally; do not edit Tone concurrently.
"""
import hashlib
import json
import os
from pathlib import Path
import selectors
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parent
RESULTS = ROOT / 'results'
TARGET = Path(os.environ.get('CARGO_TARGET_DIR', ROOT / 'target')).resolve()
BINARY = TARGET / 'debug/lifecycle-runtime'
SOURCE = ROOT / 'tone/src/lib.rs'
COMMAND = ['cargo', 'build', '--offline', '--locked', '-p', 'lifecycle-runtime', '--message-format=json']


def build(label):
    start = time.perf_counter()
    result = subprocess.run(COMMAND, cwd=ROOT, text=True, capture_output=True)
    (RESULTS / f'{label}.log').write_text(result.stderr)
    artifacts = [json.loads(line) for line in result.stdout.splitlines() if line.startswith('{')]
    return {'seconds': time.perf_counter() - start, 'exit_code': result.returncode,
            'rebuilt': [m['target']['name'] for m in artifacts if m.get('reason') == 'compiler-artifact' and not m['fresh']],
            'tone_profiles': [m['profile'] for m in artifacts if m.get('reason') == 'compiler-artifact' and m['target']['name'] == 'tone']}


def launch(folder, label):
    log = (RESULTS / f'{label}-runtime.log').open('w')
    process = subprocess.Popen([str(BINARY), str(folder)], stdin=subprocess.PIPE,
                               stdout=subprocess.PIPE, stderr=log, text=True)
    log.close()
    selector = selectors.DefaultSelector()
    selector.register(process.stdout, selectors.EVENT_READ)
    start = time.perf_counter()
    try:
        while time.perf_counter() - start < 30:
            if selector.select(0.1):
                line = process.stdout.readline()
                if line.startswith('FIRST_FRAME '):
                    return process, {'seconds': time.perf_counter() - start, 'marker': line.strip()}
            if process.poll() is not None:
                raise RuntimeError(f'Runtime exited: {process.returncode}')
        raise RuntimeError('No first frame within 30 seconds')
    except BaseException:
        process.terminate()
        process.wait(timeout=10)
        raise
    finally:
        selector.close()


def stop(process):
    process.stdin.write('quit\n')
    process.stdin.flush()
    output, _ = process.communicate(timeout=10)
    assert process.returncode == 0 and 'STOPPED' in output, (process.returncode, output)


def inspect(folder):
    return json.loads(subprocess.check_output([str(BINARY), str(folder), '--inspect'], text=True))


def hash_binary():
    return hashlib.sha256(BINARY.read_bytes()).hexdigest()


if __name__ == '__main__':
    RESULTS.mkdir(exist_ok=True)
    original = SOURCE.read_text()
    process = None
    evidence = {}
    try:
        evidence['bootstrap'] = build('reload-bootstrap')
        assert evidence['bootstrap']['exit_code'] == 0, evidence
        with tempfile.TemporaryDirectory(prefix='sound-core-reload-') as directory:
            folder = Path(directory)
            inspect(folder)  # Seed this disposable project.
            record = folder / 'state/tone-a.json'
            value = json.loads(record.read_text())
            value['state'] = {'frequency_hz': 660.0, 'gain': 0.35}
            record.write_text(json.dumps(value))
            expected = inspect(folder)
            process, evidence['initial'] = launch(folder, 'reload-initial')
            baseline_hash = hash_binary()
            SOURCE.write_text(original + '\ncompile_error!("intentional reload test");\n')
            evidence['failed_build'] = build('reload-failed')
            assert evidence['failed_build']['exit_code'] != 0
            assert process.poll() is None
            assert hash_binary() == baseline_hash
            evidence['failure_kept_process_and_binary'] = True
            started = time.perf_counter()
            changed = original.replace('* self.state.gain;', '* self.state.gain * 0.9;')
            assert changed != original, 'DSP edit pattern missing'
            SOURCE.write_text(changed)
            evidence['successful_build'] = build('reload-success')
            assert evidence['successful_build']['exit_code'] == 0
            assert process.poll() is None
            assert hash_binary() != baseline_hash
            stop(process)
            process = None
            process, evidence['replacement'] = launch(folder, 'reload-replacement')
            evidence['edit_to_frame_seconds'] = time.perf_counter() - started
            restored = inspect(folder)
            assert restored == expected
            assert not restored['playing'] and restored['undo'] == 0
            evidence['restored'] = restored
            evidence['old_runtime_survived_successful_build'] = True
            stop(process)
            process = None
    finally:
        SOURCE.write_text(original)
        if process is not None and process.poll() is None:
            stop(process)
        # Leave a binary matching restored source, not the temporary DSP variant.
        evidence['restore_build'] = build('reload-restore')
        (RESULTS / 'reload.json').write_text(json.dumps(evidence, indent=2) + '\n')
    assert evidence['restore_build']['exit_code'] == 0
    print(json.dumps(evidence, indent=2))
