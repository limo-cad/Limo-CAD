#!/usr/bin/env python3
"""Exercise the actual overlay source, with and without its gzwrite repair."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import resource
import shlex
import subprocess
import tempfile
import urllib.request


def run(command, **kwargs):
    return subprocess.run(command, check=True, timeout=120, **kwargs)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--archive", type=Path, help="use an already downloaded source archive")
    args = parser.parse_args()
    # Expected negative-control crashes should not leave core dumps behind.
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    port = Path(__file__).resolve().parent
    manifest = json.loads((port / "vcpkg.json").read_text())
    recipe = (port / "portfile.cmake").read_text()
    checksum = re.search(r"SHA512 ([0-9a-f]{128})", recipe).group(1)
    patches = re.search(r"PATCHES\s+(.*?)\n\)", recipe, re.S).group(1).split()
    repair = "CVE-2026-76844.patch"  # Upstream vcpkg filename; fixes CVE-2026-85091.
    if repair not in patches:
        raise RuntimeError("the selected zlib port does not apply the gzwrite repair")
    version = manifest["version"]
    with tempfile.TemporaryDirectory(prefix="zlib-regression-") as temporary:
        work = Path(temporary)
        archive = args.archive
        if archive is None:
            archive = work / "source.tar.gz"
            url = f"https://github.com/madler/zlib/archive/refs/tags/v{version}.tar.gz"
            with urllib.request.urlopen(url, timeout=60) as response:
                archive.write_bytes(response.read())
        if hashlib.sha512(archive.read_bytes()).hexdigest() != checksum:
            raise RuntimeError("zlib source archive does not match the selected port's SHA512")
        run(["tar", "-xf", str(archive.resolve()), "-C", str(work)])
        source = work / f"zlib-{version}"
        for patch in patches:
            run(["patch", "--batch", "-p1", "-i", str(port / patch)], cwd=source)
        compiler = shlex.split(os.environ.get("CC", "cc"))
        flags = ["-g", "-O1", "-fsanitize=address", "-fno-omit-frame-pointer",
                 "-DHAVE_UNISTD_H", "-I", str(source)]
        sources = [str(path) for path in sorted(source.glob("*.c"))]
        environment = dict(os.environ, ASAN_OPTIONS="detect_leaks=0:abort_on_error=1:symbolize=0")

        def compile_probe(name, test):
            executable = work / name
            run(compiler + flags + [str(test)] + sources + ["-o", str(executable)])
            return executable

        fixed = compile_probe("fixed", port / "nonblocking-write.c")
        for mode in ("wb", "wbT"):
            run([str(fixed), mode], cwd=work, env=environment)
        example = compile_probe("example", source / "test" / "example.c")
        run([str(example)], cwd=work, env=environment)

        # A passing positive case alone could silently stop exercising the bug.
        # Remove only the repair, rebuild, and require an ASan failure per mode.
        run(["patch", "--batch", "-R", "-p1", "-i", str(port / repair)], cwd=source)
        vulnerable = compile_probe("vulnerable", port / "nonblocking-write.c")
        for mode in ("wb", "wbT"):
            result = subprocess.run([str(vulnerable), mode], cwd=work, env=environment,
                                    capture_output=True, text=True, timeout=30)
            if result.returncode == 0 or "ERROR: AddressSanitizer:" not in result.stderr:
                raise RuntimeError(f"negative control did not expose the bug ({mode}):\n{result.stderr}")
            print(f"Unpatched {mode}: AddressSanitizer detected the invalid write")
        print(f"zlib {version}#{manifest['port-version']}: regression and upstream example passed")


if __name__ == "__main__":
    main()
