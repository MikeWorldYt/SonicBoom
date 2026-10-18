#!/usr/bin/env python3
"""Fast, static CUDA configuration check. NOT a CUDA build.

This runs in ordinary pull-request CI and proves the CUDA build is *wired
correctly* (features, Docker stages, pinned images, native CMake options,
NCCL policy and runtime packaging). It never invokes NVCC, CUDA or Docker, so
it says nothing about GPU inference working. The real compile lives in the
dedicated `cuda-full.yml` workflow.
"""
from pathlib import Path
import re
import tomllib

ROOT = Path(__file__).resolve().parents[2]


def check_cargo(manifest):
    features = manifest.get('features', {})
    for name in ('cuda', 'qwen-gguf', 'qwen-gguf-cuda'):
        if name not in features:
            raise ValueError(f'Cargo.toml is missing the `{name}` feature')
    if 'ort/cuda' not in features['cuda']:
        raise ValueError('`cuda` must enable `ort/cuda`')
    if 'sonicboom-qwen-llama/cuda' not in features['qwen-gguf-cuda']:
        raise ValueError('`qwen-gguf-cuda` must enable `sonicboom-qwen-llama/cuda`')
    if 'qwen-gguf' not in features['qwen-gguf-cuda']:
        raise ValueError('`qwen-gguf-cuda` must build on `qwen-gguf`')


def check_dockerfile(text):
    if 'AS builder-cuda' not in text:
        raise ValueError('Dockerfile must define a `builder-cuda` stage')
    if '--features server,cuda,qwen-gguf-cuda,pocket-tts' not in text:
        raise ValueError('builder-cuda must select explicit CUDA features')
    if 'COPY --from=builder-cuda' not in text:
        raise ValueError('a runtime stage must COPY from builder-cuda')
    if 'AS runtime-cuda' not in text:
        raise ValueError('Dockerfile must define a `runtime-cuda` runtime stage')
    # Every CUDA base image must be digest-pinned.
    cuda_froms = [line for line in text.splitlines()
                  if line.startswith('FROM ') and 'cuda' in line]
    if not cuda_froms:
        raise ValueError('no CUDA base image found')
    for line in cuda_froms:
        if '@sha256:' not in line:
            raise ValueError(f'CUDA base image is not digest-pinned: {line}')
    for provider in ('libonnxruntime_providers_shared.so',
                     'libonnxruntime_providers_cuda.so'):
        if provider not in text:
            raise ValueError(f'Dockerfile does not verify/stage {provider}')
    if '/app/ort-lib' not in text:
        raise ValueError('CUDA image must stage providers under /app/ort-lib')
    # CUDA runtime must fail on unresolved libraries, allowing only libcuda.
    if "libcuda\\.so\\.1 => not found" not in text:
        raise ValueError('CUDA runtime must allow only libcuda.so.1 to be absent')
    # Every required COPY input must exist relative to the build context.
    sources = re.findall(r'^COPY\s+(?!.*--from=)([^\n]+)$', text, flags=re.MULTILINE)
    required_inputs = {'Cargo.toml', 'native', 'vendor', 'src', 'build.rs'}
    copied = set()
    for line in sources:
        copied.update(token.lstrip('./') for token in line.split()[:-1])
    missing = required_inputs - copied
    if missing:
        raise ValueError(f'Dockerfile never COPYs required inputs: {sorted(missing)}')


def check_native(cmake_text, build_rs_text):
    if 'SONICBOOM_CUDA_NCCL' not in cmake_text:
        raise ValueError('CMakeLists.txt must expose the SONICBOOM_CUDA_NCCL option')
    option = re.search(r'option\(\s*SONICBOOM_CUDA_NCCL\s+"[^"]*"\s+(\w+)\s*\)', cmake_text)
    if not option:
        raise ValueError('SONICBOOM_CUDA_NCCL must be declared with `option(...)`')
    if option[1].upper() != 'OFF':
        raise ValueError('SONICBOOM_CUDA_NCCL must default OFF to keep ggml-cuda '
                         'from referencing unlinked NCCL symbols')
    if 'set(GGML_CUDA_NCCL' not in cmake_text:
        raise ValueError('CMakeLists.txt must force GGML_CUDA_NCCL from the option')
    if 'cuda-link-paths.txt' not in cmake_text:
        raise ValueError('CMakeLists.txt must export resolved CUDA library paths')
    if 'SONICBOOM_CUDA_ARCHITECTURES' not in cmake_text:
        raise ValueError('CMakeLists.txt must expose the CUDA architecture override')
    if 'SONICBOOM_CUDA_NCCL' not in build_rs_text:
        raise ValueError('build.rs must forward SONICBOOM_CUDA_NCCL to CMake')
    if 'rustc-link-lib=nccl' not in build_rs_text:
        raise ValueError('build.rs must link NCCL only when the opt-in is enabled')
    if 'find_package(NCCL REQUIRED)' not in cmake_text:
        raise ValueError('CMakeLists.txt must require NCCL when the opt-in is enabled')


def validate(root=ROOT):
    manifest = tomllib.loads((root / 'Cargo.toml').read_text())
    check_cargo(manifest)
    check_dockerfile((root / 'Dockerfile').read_text())
    check_native(
        (root / 'native/qwen-llama/CMakeLists.txt').read_text(),
        (root / 'native/qwen-llama/build.rs').read_text(),
    )


def main():
    validate()
    print('CUDA configuration check (static, no NVCC/Docker): OK')


if __name__ == '__main__':
    main()
