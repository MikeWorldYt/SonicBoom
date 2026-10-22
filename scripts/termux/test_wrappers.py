"""Exercise wrapper selection, integrity policy and atomic upgrades in isolation.

Uses fake package/build/ELF tools; never installs packages or overwrites the real
Termux installation. Real runtime inference is covered by test.sh separately.
"""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


class Wrappers(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.scripts = self.root / 'scripts/termux'
        self.scripts.mkdir(parents=True)
        for name in ('common.sh', 'run.sh', 'install.sh'):
            shutil.copyfile(Path(__file__).parent / name, self.scripts / name)
        self.prefix = self.root / 'prefix'
        self.bin = self.prefix / 'bin'
        self.bin.mkdir(parents=True)
        self.env = dict(os.environ, PREFIX=str(self.prefix),
                        ORT_DYLIB_PATH=str(self.root / 'custom.so'),
                        SONICBOOM_ALLOW_CUSTOM_ORT='1')
        self.env.pop('SONICBOOM_BINARY', None)
        self.library = Path(self.env['ORT_DYLIB_PATH'])
        self.library.write_bytes(b'fake Android ELF for wrapper policy tests')
        self.script(self.bin / 'uname', 'case "$1" in -m) echo aarch64;; -o) echo Android;; esac')
        self.script(self.bin / 'pkg', 'exit 0')
        self.script(self.bin / 'readelf', '''case "$1" in
-h) printf 'Machine: AArch64\\nClass: ELF64\\n';;
-d) echo '[libandroid.so]';;
--dyn-syms) echo OrtGetApiBase;;
esac''')
        self.script(self.scripts / 'build.sh', 'exit 0')
        self.release = self.root / 'target/release/SonicBoom'
        self.release.parent.mkdir(parents=True)
        self.script(self.release, 'echo new-binary; exit "${SELF_TEST_EXIT:-0}"')
        self.installed = self.bin / 'sonicboom'

    @staticmethod
    def script(path, body):
        path.write_text('#!/system/bin/sh\n' + body + '\n')
        path.chmod(0o700)

    def run_script(self, name, *args, success=True):
        result = subprocess.run(['/system/bin/sh', str(self.scripts / name), *args],
                                env=self.env, capture_output=True, text=True)
        self.assertEqual(result.returncode == 0, success, result.stdout + result.stderr)
        return result

    def test_explicit_missing_empty_or_nonexecutable_never_falls_back(self):
        for binary in (str(self.root / 'missing'), '', str(self.library)):
            with self.subTest(binary=binary):
                self.env['SONICBOOM_BINARY'] = binary
                result = self.run_script('run.sh', success=False)
                self.assertIn('SONICBOOM_BINARY', result.stderr)
                self.assertNotIn('new-binary', result.stdout)

    def test_default_and_explicit_binary_selection(self):
        self.assertIn('new-binary', self.run_script('run.sh', '--build-info').stdout)
        self.script(self.installed, 'echo installed-binary')
        self.assertIn('installed-binary', self.run_script('run.sh').stdout)
        self.env['SONICBOOM_BINARY'] = str(self.release)
        output = self.run_script('run.sh').stdout
        self.assertIn('new-binary', output)
        self.assertNotIn('installed-binary', output)

    def test_custom_runtime_requires_explicit_opt_in(self):
        self.env.pop('SONICBOOM_ALLOW_CUSTOM_ORT')
        self.assertIn('integrity check failed', self.run_script('run.sh', success=False).stderr)
        self.env['SONICBOOM_ALLOW_CUSTOM_ORT'] = '1'
        self.run_script('run.sh')

    def test_default_runtime_never_skips_hash(self):
        default = self.prefix / 'lib/sonicboom/libonnxruntime.so'
        default.parent.mkdir(parents=True)
        shutil.copyfile(self.library, default)
        self.env.pop('ORT_DYLIB_PATH')
        self.assertIn('integrity check failed', self.run_script('run.sh', success=False).stderr)

    def test_install_idempotent_and_upgrade_requires_flag(self):
        self.run_script('install.sh')
        self.assertEqual(self.installed.read_bytes(), self.release.read_bytes())
        self.run_script('install.sh')
        self.script(self.release, 'echo upgraded-binary')
        before = self.installed.read_bytes()
        self.assertIn('--upgrade', self.run_script('install.sh', success=False).stderr)
        self.assertEqual(self.installed.read_bytes(), before)
        self.run_script('install.sh', '--upgrade')
        self.assertEqual(self.installed.read_bytes(), self.release.read_bytes())
        self.assertEqual(list(self.bin.glob('.sonicboom.*')), [])

    def test_failed_staged_self_test_preserves_installation(self):
        self.script(self.installed, 'echo old-binary')
        before = self.installed.read_bytes()
        self.env['SELF_TEST_EXIT'] = '1'
        self.run_script('install.sh', '--upgrade', success=False)
        self.assertEqual(self.installed.read_bytes(), before)
        self.assertEqual(list(self.bin.glob('.sonicboom.*')), [])

    def test_upgrade_rejects_invalid_runtime_before_replacement(self):
        self.script(self.installed, 'echo old-binary')
        before = self.installed.read_bytes()
        self.env.pop('SONICBOOM_ALLOW_CUSTOM_ORT')
        self.run_script('install.sh', '--upgrade', success=False)
        self.assertEqual(self.installed.read_bytes(), before)
        self.assertEqual(list(self.bin.glob('.sonicboom.*')), [])

    def test_unknown_install_arguments_fail(self):
        self.run_script('install.sh', '--unknown', success=False)
        self.run_script('install.sh', '--upgrade', 'extra', success=False)
        self.assertFalse(self.installed.exists())


if __name__ == '__main__':
    unittest.main()
