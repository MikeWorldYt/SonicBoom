"""Regression tests for final-byte checksums and archive safety properties."""
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

    def test_archives_are_deterministic_allowlisted_and_case_correct(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / 'docs').mkdir()
            for path in ('LICENSE', 'README.md', 'docs/RELEASING.md', 'models.sha256.json'):
                (root / path).write_text('documentation')
            (root / 'tokens.json').write_text('must never ship')
            for target, name, asset in [
                ('x86_64-unknown-linux-gnu', 'SonicBoom', package.ASSETS[0]),
                ('x86_64-pc-windows-msvc', 'SonicBoom.exe', package.ASSETS[1]),
            ]:
                build = root / 'target' / target / 'release'
                build.mkdir(parents=True)
                # Lowercase executable must fail, even on a case-insensitive
                # host by using an unrelated filename and no correct binary.
                (build / 'wrong-name').write_bytes(b'bad')
                with patch.object(package, 'ROOT', root):
                    with self.assertRaises(RuntimeError):
                        package.package(target, root / 'missing')
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
                self.assertNotIn('tokens.json', names)
                self.assertIn('LICENSE', names)


if __name__ == '__main__':
    unittest.main()
