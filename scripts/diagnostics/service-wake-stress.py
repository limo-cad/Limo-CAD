#!/usr/bin/env python3
"""Run bounded wake probes in a temporary Bevy copy; never create OS windows.

Example: python3 scripts/diagnostics/service-wake-stress.py --out /tmp/wake-proof
Linux needs an X11 display (e.g. run under xvfb-run), or --backend wayland.
The output includes the copied sources, compiler log and before/after receipts.
"""
import argparse
import os
from pathlib import Path
import shutil
import subprocess
import sys

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--out", type=Path, required=True)
parser.add_argument("--target-dir", type=Path)
parser.add_argument("--backend", choices=["x11", "wayland"], default="x11")
parser.add_argument("--offline", action="store_true")
args = parser.parse_args()
repo = Path(__file__).resolve().parents[2]
out = args.out.resolve()
out.mkdir(parents=True, exist_ok=True)
crate = out / "bevy_winit"
# Refuse to overwrite an earlier probe or unrelated files.
shutil.copytree(repo / "desktop/vendor/bevy_winit", crate)
shutil.copyfile(repo / "desktop/Cargo.lock", crate / "Cargo.lock")
(crate / "examples").mkdir()
(crate / "src/state").mkdir()
shutil.copyfile(Path(__file__).with_suffix(".rs"), crate / "src/state/wake_probe.rs")
shutil.copyfile(repo / "desktop/src/native_viewport/service_wake.rs", crate / "src/state/service_wake.rs")
with (crate / "src/lib.rs").open("a") as f:
    f.write("\npub use state::wake_probe::run;\n")
with (crate / "src/state.rs").open("a") as f:
    f.write("\npub(crate) mod wake_probe;\n")
with (crate / "Cargo.toml").open("a") as f:
    f.write('\n[[example]]\nname = "wake_stress"\npath = "examples/wake_stress.rs"\n')
(crate / "examples/wake_stress.rs").write_text("fn main() { bevy_winit::run(); }\n")
env = dict(os.environ, CARGO_PROFILE_DEV_DEBUG="0", CARGO_INCREMENTAL="0")
target = (args.target_dir or out / "target").resolve()
env["CARGO_TARGET_DIR"] = str(target)
command = ["cargo", "build", "--manifest-path", str(crate / "Cargo.toml"),
           "--example", "wake_stress", "--no-default-features"]
if args.offline:
    command.append("--offline")
if sys.platform.startswith("linux"):
    command.extend(["--features", args.backend])
with (out / "build.log").open("w") as log:
    subprocess.run(command, env=env, stdout=log, stderr=subprocess.STDOUT, check=True)
binary = target / "debug/examples" / ("wake_stress.exe" if os.name == "nt" else "wake_stress")
for limited in [False, True]:
    run_env = dict(env)
    run_env.pop("LIMIT_SERVICE_WAKES", None)
    if limited:
        run_env["LIMIT_SERVICE_WAKES"] = "1"
    for mode in ["burst10", "single-system-once", "ten-systems-once", "burst", "feedback", "disabled"]:
        result = subprocess.run([str(binary), mode], env=run_env, capture_output=True,
                                text=True, timeout=15)
        receipt = out / f'{"limited" if limited else "unlimited"}-{mode}.log'
        receipt.write_text(result.stdout + result.stderr)
        print(result.stdout.strip(), flush=True)
        result.check_returncode()
