import hashlib
import json
from pathlib import Path
import runpy
import tempfile
import unittest
import zipfile

verify = runpy.run_path(str(Path(__file__).with_name("verify-default-windows-package.py")))["verify"]


class DefaultWindowsPackageTests(unittest.TestCase):
    def package(self, root, arch="x64", changes=None, suffix="", executable=b"fixture"):
        name = f"Limo-CAD-1.2.3-windows-{arch}"
        path = root / f"{name}{suffix}.zip"
        manifest = {
            "schema_version": 1,
            "version": "1.2.3",
            "target": f"{'x86_64' if arch == 'x64' else 'aarch64'}-pc-windows-msvc",
            "source_revision": "a" * 40,
            "source_modified": False,
            "native_computer_control": False,
            "executable_sha256": hashlib.sha256(b"fixture").hexdigest(),
        }
        manifest.update(changes or {})
        with zipfile.ZipFile(path, "w") as archive:
            archive.writestr(f"{name}/package-manifest.json", json.dumps(manifest))
            archive.writestr(f"{name}/Limo-CAD.exe", executable)
        return path

    def test_default_architectures_and_adversarial_identities(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for arch in ("x64", "arm64"):
                self.assertEqual(verify(self.package(root, arch), "1.2.3", "a" * 40), arch)
                for changes in (
                    {"native_computer_control": True},  # Includes a renamed opt-in ZIP.
                    {"native_computer_control": "false"},
                    {"native_computer_control": 0},
                    {"source_modified": True},
                    {"source_revision": "b" * 40},
                    {"version": "1.2.2"},
                    {"target": "wrong-target"},
                    {"executable_sha256": "0" * 64},
                ):
                    with self.subTest(arch=arch, changes=changes), self.assertRaises(ValueError):
                        verify(self.package(root, arch, changes), "1.2.3", "a" * 40)
            with self.assertRaises(ValueError):
                verify(self.package(root, suffix="-computer-control"), "1.2.3", "a" * 40)
            with self.assertRaises(ValueError):
                verify(self.package(root, executable=b"different binary"), "1.2.3", "a" * 40)


if __name__ == "__main__":
    unittest.main()
