"""Verify compiler errors retain both the running app and built executable."""
import hashlib
import json
from measure import BINARY, RESULTS, SOURCE, build, launch, stop

def digest():
    return hashlib.sha256(BINARY.read_bytes()).hexdigest()

original = SOURCE.read_text()
process = None
try:
    baseline = build('restored-baseline')
    assert baseline['exit_code'] == 0, baseline
    noop = build('no-op')
    assert noop['exit_code'] == 0 and noop['rebuilt'] == [] and noop['links'] == [], noop
    process, initial = launch('failure-before')
    before = digest()
    SOURCE.write_text(original + '\ncompile_error!("Intentional experiment failure");\n')
    failed = build('intentional-failure')
    unchanged = before == digest()
    alive = process.poll() is None
    assert failed['exit_code'] != 0 and unchanged and alive
    stop(process)
    process, restarted = launch('failure-after')
    result = {'baseline': baseline, 'noop': noop, 'failed_build': failed,
              'binary_sha256_before': before, 'binary_sha256_after': digest(),
              'binary_unchanged': unchanged, 'previous_runtime_alive_after_failure': alive,
              'previous_binary_relaunch': restarted}
    (RESULTS / 'failure-check.json').write_text(json.dumps(result, indent=2))
    print(json.dumps(result), flush=True)
finally:
    SOURCE.write_text(original)
    if process is not None and process.poll() is None:
        stop(process)
