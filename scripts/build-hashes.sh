#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
mkdir -p artifacts
python3 - <<'PY'
from pathlib import Path
import hashlib
candidates=[]
for name in ['target/release/qrtransceiver','target/release/qrtx-desktop','target/release/qrtransceiver.exe','target/release/qrtx-desktop.exe']:
    p=Path(name)
    if p.is_file(): candidates.append(p)
root=Path('target/release/bundle')
if root.exists():
    candidates.extend(p for p in root.rglob('*') if p.is_file() and p.suffix in ['.deb','.AppImage','.exe','.msi','.rpm'])
candidates.extend(Path('android/app/build/outputs/apk').rglob('*.apk'))
if not candidates: raise SystemExit('No release artifacts found. Build first.')
lines=[]
for path in sorted(candidates):
    h=hashlib.sha256()
    with path.open('rb') as f:
        for block in iter(lambda:f.read(1024*1024),b''): h.update(block)
    lines.append(f'{h.hexdigest()}  {path.as_posix()}')
Path('artifacts/SHA256SUMS').write_text('\n'.join(lines)+'\n')
print('\n'.join(lines))
PY
