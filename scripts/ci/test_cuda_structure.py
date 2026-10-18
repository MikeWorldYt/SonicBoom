"""Regression checks for the static CUDA configuration guard."""
import unittest

from cuda_structure import (
    ROOT,
    check_cargo,
    check_dockerfile,
    check_native,
    validate,
)

GOOD_DOCKERFILE = '''\
FROM nvidia/cuda:13.0.2-devel-ubuntu24.04@sha256:aaaa AS builder-cuda
COPY Cargo.toml Cargo.lock ./
COPY examples ./examples
COPY native ./native
COPY vendor ./vendor
RUN cargo build --release --locked --no-default-features --features server,cuda,qwen-gguf-cuda,pocket-tts
FROM nvidia/cuda:13.0.2-cudnn-runtime-ubuntu24.04@sha256:bbbb AS runtime-cuda
COPY --from=builder-cuda /build/target/release/SonicBoom /app/sonicboom
ENV LD_LIBRARY_PATH=/app/ort-lib:/app
RUN test -f libonnxruntime_providers_shared.so
RUN test -f libonnxruntime_providers_cuda.so
RUN ldd /app/sonicboom | grep 'not found' | grep -vE '^[[:space:]]*libcuda\\.so\\.1 => not found$'
COPY src ./src
COPY build.rs ./build.rs
'''

GOOD_CMAKE = '''\
option(SONICBOOM_CUDA_NCCL "Enable NCCL" OFF)
set(GGML_CUDA_NCCL ${SONICBOOM_CUDA_NCCL} CACHE BOOL "" FORCE)
set(SONICBOOM_CUDA_ARCHITECTURES "" CACHE STRING "arch override")
file(GENERATE OUTPUT "cuda-link-paths.txt" CONTENT "${X}")
find_package(NCCL REQUIRED)
'''

GOOD_BUILD_RS = 'cmake.define("SONICBOOM_CUDA_NCCL", "OFF");\nprintln!("cargo:rustc-link-lib=nccl");\n'


class CudaStructureTests(unittest.TestCase):
    def test_real_repository_passes(self):
        validate(ROOT)

    def test_missing_cuda_feature(self):
        with self.assertRaisesRegex(ValueError, 'missing the `cuda` feature'):
            check_cargo({'features': {}})

    def test_dockerfile_missing_stage(self):
        with self.assertRaisesRegex(ValueError, 'builder-cuda'):
            check_dockerfile(GOOD_DOCKERFILE.replace('AS builder-cuda', 'AS builder'))

    def test_dockerfile_unpinned_cuda_image(self):
        broken = GOOD_DOCKERFILE.replace('@sha256:aaaa', '')
        with self.assertRaisesRegex(ValueError, 'digest-pinned'):
            check_dockerfile(broken)

    def test_dockerfile_missing_copy_input(self):
        broken = GOOD_DOCKERFILE.replace('COPY native ./native\n', '')
        with self.assertRaisesRegex(ValueError, 'never COPYs required inputs'):
            check_dockerfile(broken)

    def test_nccl_default_must_be_off(self):
        broken = GOOD_CMAKE.replace('"Enable NCCL" OFF', '"Enable NCCL" ON')
        with self.assertRaisesRegex(ValueError, 'default OFF'):
            check_native(broken, GOOD_BUILD_RS)

    def test_nccl_opt_in_must_be_forwarded(self):
        with self.assertRaisesRegex(ValueError, 'forward SONICBOOM_CUDA_NCCL'):
            check_native(GOOD_CMAKE, 'nothing here')


if __name__ == '__main__':
    unittest.main()
