#!/usr/bin/env python3
"""Retain an ordered GIF of actual native fixture checkpoints, including partial failures."""
import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess
import sys


def main():
    root = Path(sys.argv[1]).resolve(strict=True)
    manifest = root / "frames.json"
    if not manifest.is_file() or manifest.stat().st_size > 65536:
        raise RuntimeError("A bounded native frames.json is required")
    data = json.loads(manifest.read_text(encoding="utf-8"))
    frames = data["frames"]
    if data.get("version") != 1 or not 2 <= len(frames) <= 16:
        raise RuntimeError("Expected 2..16 actual native frames")
    image_tool = shutil.which("magick") or shutil.which("convert")
    if not image_tool:
        raise RuntimeError("The Scripts evidence step requires installed ImageMagick")
    args = [image_tool, "-limit", "memory", "128MiB", "-limit", "map", "256MiB",
            "-limit", "disk", "128MiB"]
    for frame in frames:
        name = frame["file"]
        if not re.fullmatch(r"[a-zA-Z0-9-]+\.png", name):
            raise RuntimeError("Frame name is outside the native evidence namespace")
        path = root / name
        if path.is_symlink() or path.resolve(strict=True).parent != root:
            raise RuntimeError("Native frame redirects outside its fixture")
        if not 1024 < path.stat().st_size <= 8 * 1024 * 1024:
            raise RuntimeError("Native frame is missing or exceeds 8 MiB")
        source = path.read_bytes()
        if not source.startswith(b"\x89PNG\r\n\x1a\n") or hashlib.sha256(source).hexdigest() != frame["sha256"]:
            raise RuntimeError("Native frame bytes differ from their capture receipt")
        # Reduce each decoded native image before retaining the sequence.
        args.extend(["(", "-delay", "100", str(path), "-resize", "1280x720>",
                     "-background", "#20242a", "-gravity", "center", "-extent", "1280x720", ")"])
    output = root / "portable-source.gif"
    if output.exists():
        raise RuntimeError("Preserve the existing operational GIF")
    args.extend(["-loop", "0", "-layers", "Optimize", str(output)])
    encoded = subprocess.run(args, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE,
                             text=True, encoding="utf-8", errors="replace", timeout=45)
    if encoded.returncode:
        raise RuntimeError(f"ImageMagick failed: {encoded.stderr[:2048]}")
    if not 1024 < output.stat().st_size <= 8 * 1024 * 1024:
        raise RuntimeError("Operational GIF is empty or exceeds 8 MiB")
    if sum(path.stat().st_size for path in root.iterdir() if path.is_file()) > 64 * 1024 * 1024:
        raise RuntimeError("Portable source evidence exceeds 64 MiB")
    receipt = {"gif": output.name, "sha256": hashlib.sha256(output.read_bytes()).hexdigest(),
               "frames": frames, "state_checks_passed": data["state_checks_passed"],
               "desktop_build": data["desktop_build"], "github_sha": data["github_sha"],
               "github_run_id": data["github_run_id"], "github_run_attempt": data["github_run_attempt"],
               "scope": "Ordered actual native UI/owned chooser checkpoints; not a continuous pointer recording"}
    (root / "gif.json").write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
