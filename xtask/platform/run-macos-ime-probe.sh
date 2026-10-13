#!/usr/bin/env bash
set -euo pipefail
[[ ${GITHUB_ACTIONS:-} == true && ${RUNNER_OS:-} == macOS &&
   ${RUNNER_ENVIRONMENT:-} == github-hosted &&
   ${GITHUB_REPOSITORY_ID:-} == 1313334315 &&
   ${GITHUB_RUN_ID:-} =~ ^[0-9]+$ ]] || { echo 'Disposable GitHub macOS runner required' >&2; exit 1; }
[[ $# == 1 ]] || { echo 'Expected a fresh absolute evidence directory' >&2; exit 1; }
probe_out=$1
python3 - "$probe_out" <<'PY'
import json, os, pathlib, sys
out = pathlib.Path(sys.argv[1])
root = pathlib.Path(os.environ['RUNNER_TEMP']).resolve()
if not out.is_absolute() or not out.resolve().is_relative_to(root) or out.resolve() == root:
    raise SystemExit('Evidence must be beneath RUNNER_TEMP')
if out.exists() and any(out.iterdir()):
    raise SystemExit('Preserve prior evidence: use an empty directory')
out.mkdir(parents=True, exist_ok=True)
(out / 'report.json').write_text(json.dumps({'status': 'compiling', 'native_bevy_validated': False}))
PY
finish() {
  probe_exit=$?
  python3 - "$probe_out/report.json" "$probe_exit" <<'PY'
import json, pathlib, sys
p = pathlib.Path(sys.argv[1]); report = json.loads(p.read_text())
report['process_exit_code'] = int(sys.argv[2])
if int(sys.argv[2]) != 0 or report['status'] in ('compiling', 'started', 'exercise-in-progress'):
    report['interrupted_phase'] = report['status']; report['status'] = 'failed'
    report.setdefault('error', 'Probe did not finish; inspect compile.log, launch.json, launch.log, and probe.log')
p.write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n')
PY
  exit "$probe_exit"
}
trap finish EXIT
probe_dir=$(cd -- "$(dirname -- "$0")" && pwd)
xcrun swiftc -framework AppKit -framework Carbon -framework CoreGraphics \
  "$probe_dir/macos-ime-probe.swift" -o "$probe_out/macos-ime-probe" 2>&1 | tee "$probe_out/compile.log"
shasum -a 256 "$probe_out/macos-ime-probe" "$probe_dir/macos-ime-probe.swift" > "$probe_out/hashes.txt"
probe_args=(--out "$probe_out")
[[ ${PROBE_PROVISION:-false} != true ]] || probe_args+=(--enable-japanese)
[[ ${PROBE_EXERCISE:-false} != true ]] || probe_args+=(--exercise)
if [[ ${PROBE_EXERCISE:-false} == true ]]; then
  python3 - "$probe_out" <<'PY'
import os, pathlib, plistlib, shutil, sys
out = pathlib.Path(sys.argv[1])
contents = out / 'StockIMEProbe.app' / 'Contents'
(contents / 'MacOS').mkdir(parents=True)
shutil.copy2(out / 'macos-ime-probe', contents / 'MacOS' / 'macos-ime-probe')
with (contents / 'Info.plist').open('wb') as f:
    plistlib.dump({
        'CFBundleIdentifier': 'org.nobscad.qa.StockIMEProbe.run' + os.environ['GITHUB_RUN_ID'],
        'CFBundleExecutable': 'macos-ime-probe', 'CFBundlePackageType': 'APPL',
        'CFBundleName': 'Limo CAD disposable IME probe', 'CFBundleVersion': '1',
        'CFBundleInfoDictionaryVersion': '6.0', 'NSPrincipalClass': 'NSApplication',
        'NSHighResolutionCapable': True, 'LSMinimumSystemVersion': '14.0',
    }, f)
PY
  plutil -lint "$probe_out/StockIMEProbe.app/Contents/Info.plist"
  shasum -a 256 "$probe_out/StockIMEProbe.app/Contents/Info.plist" \
    "$probe_out/StockIMEProbe.app/Contents/MacOS/macos-ime-probe" >> "$probe_out/hashes.txt"
  "$probe_out/macos-ime-probe" "${probe_args[@]}" --launch 2>&1 | tee "$probe_out/launch.log"
else
  "$probe_out/macos-ime-probe" "${probe_args[@]}" 2>&1 | tee "$probe_out/probe.log"
fi
