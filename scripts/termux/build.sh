#!/system/bin/sh
. "$(dirname "$0")/common.sh"
# Use Termux tool RUNPATHs; inherited private DSOs can break CMake or Android ORT.
unset LD_LIBRARY_PATH
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}"
[ "$(rustc -vV | sed -n 's/^host: //p')" = aarch64-linux-android ] || fail 'Install native Termux Rust (pkg install rust)'
for tool in cargo clang clang++ cmake make pkg-config curl unzip readelf; do
    command -v "$tool" >/dev/null || fail "Missing build tool: $tool"
done
clang --version >/dev/null
cmake --version >/dev/null
pkg-config --exists openssl opus || fail 'Install openssl and libopus'
prepare_ort
cd "$ROOT"
# One can opt into Pocket/Qwen CPU without changing the default build.
features=server,android-ort
case "${SONICBOOM_TERMUX_PROVIDER:-supertonic}" in
    supertonic) ;;
    pocket) features=$features,pocket-tts ;;
    qwen) git submodule update --init --recursive; features=$features,qwen-gguf ;;
    *) fail 'Provider must be supertonic, pocket, or qwen' ;;
esac
cargo build --release --locked --no-default-features --features "$features" --bin SonicBoom
env -u LD_LIBRARY_PATH "$ROOT/target/release/SonicBoom" --onnx-self-test
printf 'Binary: %s\nRuntime: %s\n' "$ROOT/target/release/SonicBoom" "$ORT_DYLIB_PATH"
