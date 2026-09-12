"""Temporarily optimize the edited extension, render DSP, and time seven edits."""
import json
import shutil

import measure

ROOT = measure.ROOT
manifest = ROOT / 'Cargo.toml'
host = ROOT / 'runtime/src/main.rs'
dsp = ROOT / 'extension/src/optimized_dsp.rs'
originals = {path: path.read_bytes() for path in (manifest, host, measure.SOURCE)}
rows = []
process = None

if dsp.exists():
    raise RuntimeError(f'Refusing to overwrite {dsp}')

try:
    manifest.write_text(manifest.read_text() + '\n[profile.dev.package.timing-extension]\nopt-level = 3\n')
    measure.SOURCE.write_text('pub mod optimized_dsp;\n' + measure.SOURCE.read_text())
    host.write_text(host.read_text().replace('fn main() {', 'fn main() {\n    timing_extension::optimized_dsp::verify_render();'))
    shutil.copyfile(ROOT / 'optimized_dsp.rs', dsp)
    initial_source = measure.SOURCE.read_text()
    initial_dsp = dsp.read_text()
    bootstrap = measure.build('optimized-bootstrap')
    (measure.RESULTS / 'optimized-bootstrap.json').write_text(json.dumps(bootstrap, indent=2))
    assert bootstrap['exit_code'] == 0, bootstrap
    process, _ = measure.launch('optimized-initial')
    for index in range(1, 8):
        started = measure.time.perf_counter()
        gain = f'{0.20 + index / 100:.2f}'
        revision = f'optimized-{index}'
        dsp.write_text(initial_dsp.replace('0.20;', f'{gain};'))
        measure.SOURCE.write_text(measure.re.sub(
            r'pub const REVISION: &str = "[^"]+";',
            f'pub const REVISION: &str = "{revision}";', initial_source))
        built = measure.build(f'optimized-edit-{index}')
        assert built['exit_code'] == 0, built
        assert set(built['rebuilt']) == {'timing_extension', 'sound-tools-timing'}, built
        messages = [json.loads(line) for line in (measure.RESULTS / f'optimized-edit-{index}.jsonl').read_text().splitlines()]
        profile = next(m['profile'] for m in messages if m.get('reason') == 'compiler-artifact' and m['target']['name'] == 'timing_extension')
        assert profile['opt_level'] == '3', profile
        assert process.poll() is None, 'Previous runtime died during build'
        restart_started = measure.time.perf_counter()
        measure.stop(process)
        process, launched = measure.launch(f'optimized-edit-{index}')
        restart = measure.time.perf_counter() - restart_started
        elapsed = measure.time.perf_counter() - started
        assert revision in launched['marker'], launched
        runtime_log = (measure.RESULTS / f'optimized-edit-{index}-runtime.log').read_text()
        rendered = next(line for line in runtime_log.splitlines() if line.startswith('DSP '))
        assert f'gain={gain} ' in rendered, rendered
        row = {'run': index, 'build': built, 'extension_profile': profile,
               'launch': launched, 'dsp': rendered, 'stop_to_frame_seconds': restart,
               'edit_to_frame_seconds': elapsed, 'previous_runtime_survived_build': True}
        rows.append(row)
        (measure.RESULTS / 'optimized-measurements.json').write_text(json.dumps(rows, indent=2))
        print(json.dumps(row), flush=True)
finally:
    if process is not None and process.poll() is None:
        measure.stop(process)
    for path, content in originals.items():
        path.write_bytes(content)
    dsp.unlink(missing_ok=True)
