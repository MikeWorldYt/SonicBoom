"""Regression tests for final-byte checksums and archive safety properties."""
import hashlib
import json
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import patch
import zipfile

import package


class PackagingTests(unittest.TestCase):
    def test_checksums_require_both_archives_and_detect_tampering(self):
        with tempfile.TemporaryDirectory() as temp:
            directory = Path(temp)
            (directory / package.ASSETS[0]).write_bytes(b'linux')
            with self.assertRaises(RuntimeError):
                package.checksums(directory)
            (directory / package.ASSETS[1]).write_bytes(b'windows')
            package.checksums(directory)
            package.checksums(directory, verify=True)
            (directory / package.ASSETS[0]).write_bytes(b'changed')
            with self.assertRaises(RuntimeError):
                package.checksums(directory, verify=True)
            (directory / 'unexpected-secret').write_bytes(b'never upload')
            with self.assertRaises(RuntimeError):
                package.checksums(directory)

    def test_windows_requires_the_verified_runtime_and_its_own_notices(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            for name in ('docs', 'licenses/onnxruntime', 'native/build/download'):
                (root / name).mkdir(parents=True)
            for name in ('LICENSE', 'README.md', 'docs/RELEASING.md', 'models.sha256.json'):
                (root / name).write_text('documentation')
            target = 'x86_64-pc-windows-msvc'
            (root / 'native/build/download/dist.tsv').write_text(target + '\tdirectml\thttps://example.invalid/ms@1.28.0/windows\thash\n')
            build = root / 'target' / target / 'release'
            build.mkdir(parents=True)
            (build / 'SonicBoom.exe').write_bytes(b'executable fixture')
            metadata = {'packages': [{'id': 'ort-sys', 'name': 'ort-sys', 'version': 'fixture',
                                     'manifest_path': str(root / 'native/Cargo.toml')}]}
            def output(args, **kwargs):
                return json.dumps(metadata).encode() if args[0] == 'cargo' else 'build-input\n'
            with patch.object(package, 'ROOT', root), patch.object(package.subprocess, 'check_output', side_effect=output):
                with self.assertRaises(RuntimeError):
                    package.package(target, root / 'out')
                dll = build / 'DirectML.dll'
                dll.write_bytes(b'wrong runtime version')
                with self.assertRaises(RuntimeError):
                    package.package(target, root / 'out')
                fixture_hash = hashlib.sha256(dll.read_bytes()).hexdigest()
                with patch.object(package, 'DIRECTML_SHA256', fixture_hash):
                    with self.assertRaises(FileNotFoundError):
                        package.package(target, root / 'out')
                    (root / 'licenses/directml').mkdir()
                    (root / 'licenses/directml/LICENSE.txt').write_text('runtime terms fixture')
                    package.package(target, root / 'out')
            with zipfile.ZipFile(root / 'out' / package.ASSETS[1]) as archive:
                self.assertIn('DirectML.dll', archive.namelist())
                self.assertIn('licenses/directml/LICENSE.txt', archive.namelist())

    def test_archives_are_deterministic_allowlisted_and_case_correct(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / 'docs').mkdir()
            for path in ('LICENSE', 'README.md', 'docs/RELEASING.md', 'models.sha256.json'):
                (root / path).write_text('documentation')
            (root / 'vendor/llama.cpp/ggml').mkdir(parents=True)
            (root / 'vendor/llama.cpp/LICENSE').write_text('llama license fixture')
            (root / 'vendor/llama.cpp/ggml/LICENSE').write_text('ggml license fixture')
            (root / 'tokens.json').write_text('must never ship')
            for target, name, asset in [
                ('x86_64-unknown-linux-gnu', 'SonicBoom', package.ASSETS[0]),
                ('x86_64-pc-windows-msvc', 'SonicBoom.exe', package.ASSETS[1]),
            ]:
                build = root / 'target' / target / 'release'
                build.mkdir(parents=True)
                # Directory entry spelling matters even on Windows NTFS.
                lowercase = build / name.lower()
                lowercase.write_bytes(b'bad')
                with patch.object(package, 'ROOT', root):
                    with self.assertRaises(RuntimeError):
                        package.package(target, root / 'missing')
                lowercase.unlink()
                (build / name).write_bytes(b'nonempty native fixture')
                (build / 'tokens.json').write_text('must never ship')
                def output(args, **kwargs):
                    if args[0] == 'cargo':
                        return json.dumps({'packages': []}).encode()
                    return 'fixed-build-input\n'
                with patch.object(package, 'ROOT', root), patch.object(package.subprocess, 'check_output', side_effect=output):
                    package.package(target, root / 'one')
                    package.package(target, root / 'two')
                    with self.assertRaises(RuntimeError):
                        package.package(target, root / 'one')
                self.assertEqual((root / 'one' / asset).read_bytes(), (root / 'two' / asset).read_bytes())
                if name.endswith('.exe'):
                    with zipfile.ZipFile(root / 'one' / asset) as archive:
                        names = archive.namelist()
                        self.assertIn(name, names)
                else:
                    with tarfile.open(root / 'one' / asset) as archive:
                        names = archive.getnames()
                        self.assertEqual(archive.getmember(name).mode, 0o755)
                        self.assertIn('sonicboom', names)
                        self.assertEqual(archive.extractfile(name).read(), b'nonempty native fixture')
                        self.assertEqual(archive.getmember('sonicboom').mode, 0o755)
                        self.assertNotIn(b'\r', archive.extractfile('sonicboom').read())
                        self.assertIn(b'exec "$APP_DIR/SonicBoom" "$@"', archive.extractfile('sonicboom').read())
                self.assertNotIn('tokens.json', names)
                self.assertIn('LICENSE', names)
                self.assertIn('licenses/llama.cpp/LICENSE', names)
                self.assertIn('licenses/llama.cpp/ggml/LICENSE', names)
                self.assertIn('docs/RELEASING.md', names)


if __name__ == '__main__':
    unittest.main()
