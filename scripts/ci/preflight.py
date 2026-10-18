#!/usr/bin/env python3
"""Fast source/manifest checks before downloading or compiling dependencies."""
import fnmatch
from pathlib import Path
import re
import subprocess
import tomllib

ROOT = Path(__file__).resolve().parents[2]


def docker_included(path, rules):
    included = True
    for rule in rules:
        rule = rule.strip()
        if not rule or rule.startswith('#'):
            continue
        allow = rule.startswith('!')
        pattern = rule.lstrip('!').rstrip('/')
        if fnmatch.fnmatch(path, pattern) or path.startswith(pattern + '/'):
            included = allow
    return included


def validate(root=ROOT):
    manifest = tomllib.loads((root / 'Cargo.toml').read_text())
    required = ['Cargo.toml', 'Cargo.lock']
    for example in manifest.get('example', []):
        required.append(example.get('path', f"examples/{example['name']}.rs"))
    dependencies = dict(manifest.get('dependencies', {}))
    for table in manifest.get('target', {}).values():
        dependencies.update(table.get('dependencies', {}))
    for patches in manifest.get('patch', {}).values():
        dependencies.update(patches)
    for dependency in dependencies.values():
        if isinstance(dependency, dict) and 'path' in dependency:
            path = Path(dependency['path'])
            required.append(str(path / 'Cargo.toml'))
            local = tomllib.loads((root / path / 'Cargo.toml').read_text())
            required.append(str(path / local.get('lib', {}).get('path', 'src/lib.rs')))
    required.extend(['vendor/llama.cpp/include/llama.h', 'vendor/llama.cpp/CMakeLists.txt'])
    rules = (root / '.dockerignore').read_text().splitlines()
    for path in required:
        if not (root / path).is_file():
            raise ValueError(f'Missing manifest/native input: {path}; initialize pinned submodules')
        if not docker_included(path, rules):
            raise ValueError(f'Docker context excludes required input: {path}')
    stage = ''
    copies = []
    for line in (root / 'Dockerfile').read_text().splitlines():
        if line.startswith('FROM '):
            stage = line.split()[-1]
            copies = []
        if line.startswith('COPY ') and '--from=' not in line:
            copies.extend(line.split()[1:-1])
        if 'cargo fetch ' in line:
            for path in required:
                if not any(path == src or path.startswith(src.rstrip('/') + '/') for src in copies):
                    raise ValueError(f'{stage}: {path} must be copied before cargo fetch')
        if 'cargo build ' in line:
            if '--no-default-features' not in line:
                raise ValueError(f'{stage}: select container features explicitly')
            features = re.search(r'--features\s+([\w,-]+)', line)
            if not features:
                raise ValueError(f'{stage}: missing explicit features')
            selected = set(features[1].split(','))
            expected = {
                'builder': {'server', 'qwen-gguf', 'pocket-tts'},
                'builder-playback': {'server', 'playback', 'qwen-gguf', 'pocket-tts'},
                'builder-cuda': {'server', 'cuda', 'qwen-gguf-cuda', 'pocket-tts'},
            }
            for feature in features[1].split(','):
                if feature not in manifest['features']:
                    raise ValueError(f'{stage}: unknown Cargo feature {feature}')
            if stage in expected and selected != expected[stage]:
                raise ValueError(f'{stage}: expected provider/backend features {sorted(expected[stage])}')
    return required


def main():
    validate()
    status = subprocess.check_output(['git', 'submodule', 'status', '--recursive'], cwd=ROOT, text=True)
    if not status.strip() or any(line[0] != ' ' for line in status.splitlines()):
        raise ValueError(f'Submodules must match the committed gitlinks:\n{status}')
    print(status, end='')
    subprocess.run(['cargo', 'metadata', '--locked', '--no-deps', '--format-version', '1'],
                   cwd=ROOT, check=True, stdout=subprocess.DEVNULL)
    print('Pinned native sources, Cargo manifests, Docker inputs and features verified')


if __name__ == '__main__':
    main()
