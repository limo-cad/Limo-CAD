"""Reject opt-in, renamed, modified or mismatched ZIPs before release publication."""
import argparse
import hashlib
import json
from pathlib import Path
import zipfile


def verify(path: Path, version: str, revision: str) -> str:
    for arch, target in (("x64", "x86_64"), ("arm64", "aarch64")):
        name = f"Limo-CAD-{version}-windows-{arch}"
        if path.name == f"{name}.zip":
            break
    else:
        raise ValueError(f"Not a default Windows package filename: {path.name}")
    with zipfile.ZipFile(path) as archive:
        manifest_path = f"{name}/package-manifest.json"
        executable_path = f"{name}/Limo-CAD.exe"
        names = archive.namelist()
        if names.count(manifest_path) != 1 or names.count(executable_path) != 1:
            raise ValueError("Package must contain exactly one manifest and executable")
        manifest = json.loads(archive.read(manifest_path))
        expected = {
            "schema_version": 1,
            "version": version,
            "target": f"{target}-pc-windows-msvc",
            "source_revision": revision,
            "source_modified": False,
            "native_computer_control": False,
        }
        for key, value in expected.items():
            actual = manifest.get(key)
            if type(actual) is not type(value) or actual != value:
                raise ValueError(f"{path.name}: invalid release identity {key}={actual!r}")
        with archive.open(executable_path) as executable:
            digest = hashlib.file_digest(executable, "sha256").hexdigest()
        if manifest.get("executable_sha256") != digest:
            raise ValueError(f"{path.name}: executable does not match package identity")
    return arch


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", required=True)
    parser.add_argument("--revision", required=True)
    parser.add_argument("packages", type=Path, nargs="+")
    args = parser.parse_args()
    architectures = [verify(path, args.version, args.revision) for path in args.packages]
    if sorted(architectures) != ["arm64", "x64"]:
        raise ValueError("Release requires exactly one default x64 and one default ARM64 ZIP")
