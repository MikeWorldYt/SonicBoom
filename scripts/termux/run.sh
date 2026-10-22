#!/system/bin/sh
. "$(dirname "$0")/common.sh"
validate_ort
# Android system DSOs must resolve their own dependencies, not Termux liblzma.
unset LD_LIBRARY_PATH
binary=${SONICBOOM_BINARY:-$PREFIX/bin/sonicboom}
[ -x "$binary" ] || binary=$ROOT/target/release/SonicBoom
[ -x "$binary" ] || fail 'Build or install SonicBoom first'
"$binary" --onnx-self-test
exec "$binary" "$@"
