"""Regression checks for the source preflight; no third-party test framework."""
from pathlib import Path
import shutil
import tempfile
import unittest

from preflight import ROOT, docker_included, validate


class PreflightTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        for path in validate():
            dest = self.root / path
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ROOT / path, dest)
        for path in ['Dockerfile', '.dockerignore']:
            shutil.copyfile(ROOT / path, self.root / path)

    def test_repository_inputs(self):
        validate(self.root)

    def test_missing_example(self):
        (self.root / 'examples/cache_benchmark.rs').unlink()
        with self.assertRaisesRegex(ValueError, 'cache_benchmark'):
            validate(self.root)

    def test_missing_submodule(self):
        (self.root / 'vendor/llama.cpp/include/llama.h').unlink()
        with self.assertRaisesRegex(ValueError, 'initialize pinned submodules'):
            validate(self.root)

    def test_unknown_docker_feature(self):
        path = self.root / 'Dockerfile'
        path.write_text(path.read_text().replace('server,qwen-gguf,', 'server,qwen,'))
        with self.assertRaisesRegex(ValueError, 'unknown Cargo feature qwen'):
            validate(self.root)

    def test_optional_path_must_be_copied_before_fetch(self):
        path = self.root / 'Dockerfile'
        path.write_text(path.read_text().replace('COPY native ./native', ''))
        with self.assertRaisesRegex(ValueError, 'before cargo fetch'):
            validate(self.root)

    def test_cuda_must_select_native_and_onnx_backends(self):
        path = self.root / 'Dockerfile'
        path.write_text(path.read_text().replace('server,cuda,qwen-gguf-cuda,', 'server,cuda,qwen-gguf,'))
        with self.assertRaisesRegex(ValueError, 'expected provider/backend features'):
            validate(self.root)

    def test_missing_optional_path_source(self):
        (self.root / 'native/qwen-llama/src/lib.rs').unlink()
        with self.assertRaisesRegex(ValueError, 'native/qwen-llama/src/lib.rs'):
            validate(self.root)

    def test_context_must_include_examples(self):
        with (self.root / '.dockerignore').open('a') as stream:
            stream.write('\nexamples/**\n')
        with self.assertRaisesRegex(ValueError, 'context excludes'):
            validate(self.root)

    def test_credentials_and_nested_git_are_excluded(self):
        rules = (ROOT / '.dockerignore').read_text().splitlines()
        for path in ['.env', 'tokens.json', 'models/weights.onnx', 'vendor/llama.cpp/.git',
                     'native/.env', 'frontend/.env.local', 'vendor/tokens.json',
                     'native/weights.gguf', 'vendor/weights.onnx']:
            self.assertFalse(docker_included(path, rules), path)


if __name__ == '__main__':
    unittest.main()
