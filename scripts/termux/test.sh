#!/system/bin/sh
. "$(dirname "$0")/common.sh"
validate_ort
# Android system DSOs must resolve their own dependencies, not Termux liblzma.
unset LD_LIBRARY_PATH
uname -m
rustc -vV
cargo --version
binary=${SONICBOOM_BINARY:-$ROOT/target/release/SonicBoom}
"$binary" --build-info
"$binary" --onnx-self-test
command -v python3 >/dev/null || fail 'Integration tests need Python (pkg install python); the server does not'
exec python3 "$ROOT/scripts/termux/integration.py" "$binary" "$@"
