#!/usr/bin/env python3
"""Time the system linker driver without changing its arguments."""
import json
from pathlib import Path
import subprocess
import sys
import time
start = time.perf_counter()
result = subprocess.run(['/usr/bin/clang', *sys.argv[1:]])
with (Path(__file__).parent / 'results' / 'linker.jsonl').open('a') as output:
    output.write(json.dumps({'seconds': time.perf_counter() - start,
                             'exit_code': result.returncode}) + '\n')
sys.exit(result.returncode)
