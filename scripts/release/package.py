#!/usr/bin/env python3
"""Allowlisted, deterministic release archives; no models or local configuration."""
import argparse
import gzip
import hashlib
import io
import json
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import zipfile

ROOT = Path(__file__).resolve().parents[2]
ASSETS = ('sonicboom-linux-x86_64.tar.gz', 'sonicboom-windows-x86_64.zip')
DIRECTML_SHA256 = '9c9e6d822561c6c41b90e6994b3e8857cf1d66dbfb1e0c4c799c7c89b4e92da1'


def digest(path):
    with path.open('rb') as source:
        return hashlib.file_digest(source, 'sha256').hexdigest()


def package(target, output):
    windows = target == 'x86_64-pc-windows-msvc'
    if target not in ('x86_64-pc-windows-msvc', 'x86_64-unknown-linux-gnu'):
        raise ValueError(f'Unsupported release target: {target}')
    build = ROOT / 'target' / target / 'release'
    binary = build / ('SonicBoom.exe' if windows else 'SonicBoom')
    exact_names = {path.name for path in build.iterdir()} if build.is_dir() else set()
    if binary.name not in exact_names or not binary.is_file() or binary.stat().st_size == 0:
        raise RuntimeError(f'Missing/nonempty release executable required: {binary}')
    output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory() as temporary:
        stage = Path(temporary) / 'payload'
        stage.mkdir()
        shutil.copy2(binary, stage / binary.name)
        (stage / binary.name).chmod(0o755)
        for source, name in [('LICENSE', 'LICENSE'), ('README.md', 'README.md'), ('docs/RELEASING.md', 'docs/RELEASING.md'),
                             ('models.sha256.json', 'models.sha256.json')]:
            (stage / name).parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ROOT / source, stage / name)
        # Cargo's ort/copy-dylibs places required runtime libraries beside the
        # executable. Preserve SONAME names, dereferencing symlinks into files.
        for library in sorted(build.iterdir()):
            if library.is_file() and (library.suffix.lower() == '.dll' if windows else '.so' in library.name):
                shutil.copyfile(library, stage / library.name)
        if not windows:
            # Keep case-only archive names on separate filesystem paths (NTFS).
            launcher = Path(temporary) / 'sonicboom'
            launcher.write_text('#!/bin/sh\nset -eu\nAPP_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)\nexport LD_LIBRARY_PATH="$APP_DIR${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"\nexec "$APP_DIR/SonicBoom" "$@"\n', encoding='utf-8', newline='\n')
            launcher.chmod(0o755)
        # Carry upstream license/notice files for resolved target dependencies.
        metadata = json.loads(subprocess.check_output([
            'cargo', 'metadata', '--locked', '--format-version', '1',
            '--features', 'gui', '--filter-platform', target], cwd=ROOT))
        notices = stage / 'licenses'
        notices.mkdir()
        native_notices = ROOT / 'licenses/onnxruntime'
        if native_notices.is_dir():
            shutil.copytree(native_notices, notices / 'onnxruntime')
        # The native wrapper builds pinned llama.cpp/ggml outside Cargo's
        # package graph. Carry those upstream notices explicitly.
        llama = ROOT / 'vendor/llama.cpp'
        for source in sorted(llama.glob('LICENSE*')) + sorted((llama / 'ggml').rglob('LICENSE*')):
            if not source.is_file():
                continue
            destination = notices / 'llama.cpp' / source.relative_to(llama)
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(source, destination)
        inventory = []
        for pkg in sorted(metadata['packages'], key=lambda p: p['id']):
            home = Path(pkg['manifest_path']).parent
            destination = notices / f"{pkg['name']}-{pkg['version']}"
            files = [p for p in home.iterdir() if p.is_file() and
                     p.name.upper().startswith(('LICENSE', 'LICENCE', 'COPYING', 'NOTICE', 'AUTHORS', 'UNLICENSE'))]
            # Preserve notices embedded in bundled C/C++ source trees too.
            if pkg['name'] in ('knf-rs-sys', 'opusic-sys', 'esaxx-rs'):
                files += [p for p in home.rglob('*') if p.is_file() and
                          p.name.upper().startswith(('LICENSE', 'LICENCE', 'COPYING', 'NOTICE'))]
            if pkg['name'] == 'ort-sys':
                distribution = (home / 'build/download/dist.tsv').read_text()
                native_features = 'directml' if windows else 'none'
                cpu = [line for line in distribution.splitlines() if line.startswith(target + '\t' + native_features + '\t')]
                if len(cpu) != 1 or '/ms@1.28.0/' not in cpu[0] or not native_notices.is_dir():
                    raise RuntimeError('Native ORT distribution changed: update pinned licenses/onnxruntime notices')
                if windows:
                    dml = stage / 'DirectML.dll'
                    if not dml.is_file() or digest(dml) != DIRECTML_SHA256:
                        raise RuntimeError('Expected pinned DirectML 1.15.4 DLL beside executable; check ort/copy-dylibs and native notices')
                    shutil.copytree(ROOT / 'licenses/directml', notices / 'directml')
            if pkg.get('license_file'):
                files.append(home / pkg['license_file'])
            for source in sorted(set(files)):
                notice = destination / source.relative_to(home)
                notice.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(source, notice)
            inventory.append({k: pkg.get(k) for k in ('name', 'version', 'license', 'repository', 'source')})
        (notices / 'dependencies.json').write_text(json.dumps(inventory, indent=2) + '\n')
        # Native ORT distributions can carry additional notices outside the Rust
        # package. Include only notice files from the selected target build output.
        for source in sorted((build / 'build').glob('ort-sys-*/out/**/*')):
            if source.is_file() and source.name.upper().startswith(('LICENSE', 'THIRD_PARTY', 'NOTICE')):
                destination = notices / 'onnxruntime' / source.name
                destination.parent.mkdir(exist_ok=True)
                if destination.exists() and destination.read_bytes() != source.read_bytes():
                    raise RuntimeError(f'Ambiguous native notice: {source}')
                shutil.copyfile(source, destination)
        (stage / 'BUILD-INFO.json').write_text(json.dumps({
            'commit': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
            'target': target, 'features': 'default + gui',
            'rustc': subprocess.check_output(['rustc', '--version'], text=True).strip(),
        }, indent=2) + '\n')
        files = {p.relative_to(stage).as_posix(): p for p in stage.rglob('*') if p.is_file()}
        if not windows:
            files['sonicboom'] = launcher
        archive = output / ASSETS[int(windows)]
        if archive.exists():
            raise RuntimeError(f'Refusing to replace {archive}')
        if windows:
            with zipfile.ZipFile(archive, 'w', compression=zipfile.ZIP_DEFLATED, compresslevel=9) as z:
                for name, path in sorted(files.items()):
                    entry = zipfile.ZipInfo(name, (1980, 1, 1, 0, 0, 0))
                    entry.create_system = 3
                    entry.external_attr = 0o100644 << 16
                    entry.compress_type = zipfile.ZIP_DEFLATED
                    z.writestr(entry, path.read_bytes())
            with zipfile.ZipFile(archive) as z:
                assert z.testzip() is None
                assert z.getinfo('SonicBoom.exe').file_size > 0
        else:
            with archive.open('wb') as raw, gzip.GzipFile(filename='', mode='wb', fileobj=raw, mtime=0) as gz:
                with tarfile.open(fileobj=gz, mode='w') as tar:
                    for name, path in sorted(files.items()):
                        entry = tarfile.TarInfo(name)
                        entry.size = path.stat().st_size
                        entry.mode = 0o755 if path.name in ('SonicBoom', 'sonicboom') else 0o644
                        tar.addfile(entry, io.BytesIO(path.read_bytes()))
            with tarfile.open(archive) as tar:
                assert tar.getmember('SonicBoom').size > 0
                assert tar.getmember('SonicBoom').mode & 0o111
                assert tar.getmember('sonicboom').mode & 0o111
        print(f'Packaged and inspected {archive}: {digest(archive)}')


def checksums(directory, verify=False):
    actual = {p.name for p in directory.iterdir() if p.is_file()}
    if actual - {'SHA256SUMS'} != set(ASSETS):
        raise RuntimeError(f'Expected exactly both platform archives; found {sorted(actual)}')
    expected = ''.join(f'{digest(directory / name)}  {name}\n' for name in ASSETS)
    manifest = directory / 'SHA256SUMS'
    if verify:
        if manifest.read_text() != expected:
            raise RuntimeError('SHA256SUMS does not match the final archives')
    else:
        manifest.write_text(expected)
    print(expected, end='')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--target')
    parser.add_argument('--output', type=Path, default=Path('dist'))
    parser.add_argument('--checksums', action='store_true')
    parser.add_argument('--verify', action='store_true')
    args = parser.parse_args()
    if args.checksums:
        checksums(args.output, args.verify)
    elif args.target:
        package(args.target, args.output)
    else:
        parser.error('--target or --checksums required')
