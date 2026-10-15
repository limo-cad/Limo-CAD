#!/usr/bin/env python3
"""Package the read-only original-app observer; never modify or launch CAD."""
import argparse
import hashlib
import json
from pathlib import Path
import plistlib
import shutil
import subprocess

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("output", type=Path, help="New, empty output directory")
args = parser.parse_args()
output = args.output.resolve()
output.mkdir(parents=True, exist_ok=False)
source = Path(__file__).with_name("sketch-observer-macos.m")
app = output / "Limo CAD Original Cursor Recorder.app"
macos = app / "Contents" / "MacOS"
macos.mkdir(parents=True)
executable = macos / "original-cursor-recorder"
subprocess.run([
    "clang", "-fobjc-arc", "-Wall", "-Wextra", "-Werror",
    "-Wno-deprecated-declarations", "-O2", "-mmacosx-version-min=12.0",
    "-framework", "AppKit", "-framework", "ApplicationServices",
    str(source), "-o", str(executable),
], check=True)
with (app / "Contents" / "Info.plist").open("wb") as stream:
    plistlib.dump({
        "CFBundleName": "Limo CAD Original Cursor Recorder",
        "CFBundleDisplayName": "Limo CAD Original Cursor Recorder",
        "CFBundleIdentifier": "org.limocad.originalcursorrecorder",
        "CFBundleExecutable": executable.name,
        "CFBundlePackageType": "APPL",
        "CFBundleVersion": "1",
        "CFBundleShortVersionString": "1.0",
        "LSMinimumSystemVersion": "12.0",
        "NSHighResolutionCapable": True,
        "NSPrincipalClass": "NSApplication",
        "NSAccessibilityUsageDescription": "Read live drawing dimension values from the original CAD test app.",
    }, stream)
subprocess.run(["codesign", "--force", "--sign", "-", str(app)], check=True)
subprocess.run(["codesign", "--verify", "--deep", "--strict", str(app)], check=True)
result = subprocess.run([str(executable), "--self-test"], text=True, capture_output=True, check=True)
subprocess.run(["clang", "--analyze", "-fobjc-arc", "-Wall", "-Wextra", "-Werror",
                "-Wno-deprecated-declarations", "-mmacosx-version-min=12.0",
                "-Xanalyzer", "-analyzer-output=text", str(source)], check=True)
support = output / "build-support"
support.mkdir()
for path in (source, Path(__file__), source.with_suffix(".md")):
    shutil.copy2(path, support / path.name)
shutil.copy2(source.with_suffix(".md"), output / "README.md")
archive = output / "Limo CAD Original Cursor Recorder.zip"
subprocess.run(["ditto", "-c", "-k", "--sequesterRsrc", "--keepParent", str(app), str(archive)], check=True)
subprocess.run(["unzip", "-tq", str(archive)], check=True)
identity = {
    "application": str(app),
    "target_application": "Limo CAD Sketch Fix Test.app, source 71d789635750ba3d4996d062abe18d1bcc0fa41f",
    "target_engine_sha256": "71ed50af00ec242578809c4780340ea5fed4beacdaad56d6fa7edb95cc78f4be",
    "observer_source_sha256": hashlib.sha256(source.read_bytes()).hexdigest(),
    "observer_executable_sha256": hashlib.sha256(executable.read_bytes()).hexdigest(),
    "archive_sha256": hashlib.sha256(archive.read_bytes()).hexdigest(),
    "strict_compile": "passed",
    "clang_static_analysis": "passed",
    "signature": "deep strict passed",
    "archive_integrity": "passed",
    "source_checks": result.stdout.strip(),
    "live_original_app_capture": "pending user; Accessibility grant required",
    "cad_executable_changed": False,
    "cad_saved_data_changed": False,
    "input_insertion_or_mcp_requests": False,
    "pr": "https://github.com/limo-cad/Limo-CAD/pull/427",
    "pr_state": "draft",
}
(output / "build-identity.json").write_text(json.dumps(identity, indent=2) + "\n")
print(json.dumps(identity, indent=2))
