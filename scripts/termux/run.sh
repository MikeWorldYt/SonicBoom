#!/system/bin/sh
. "$(dirname "$0")/common.sh"
# Android system DSOs must resolve their own dependencies, not Termux liblzma.
unset LD_LIBRARY_PATH
if [ "${SONICBOOM_BINARY+x}" = x ]; then
    binary=$SONICBOOM_BINARY
    [ -f "$binary" ] && [ -x "$binary" ] || fail "SONICBOOM_BINARY is not an executable file: $binary"
else
    binary=$PREFIX/bin/sonicboom
    [ -x "$binary" ] || binary=$ROOT/target/release/SonicBoom
    [ -f "$binary" ] && [ -x "$binary" ] || fail 'Build or install SonicBoom first'
fi
validate_ort
"$binary" --onnx-self-test
exec "$binary" "$@"
