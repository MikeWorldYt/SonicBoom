#!/system/bin/sh
. "$(dirname "$0")/common.sh"
upgrade=0
case "${1:-}" in
    '') [ "$#" -eq 0 ] || fail 'Usage: install.sh [--upgrade]' ;;
    --upgrade) [ "$#" -eq 1 ] || fail 'Usage: install.sh [--upgrade]'; upgrade=1 ;;
    *) fail 'Usage: install.sh [--upgrade]' ;;
esac
pkg update
pkg install -y rust clang cmake make pkg-config openssl libopus git curl unzip binutils
"$ROOT/scripts/termux/build.sh"
destination=$PREFIX/bin/sonicboom
if [ -e "$destination" ] && ! cmp -s "$ROOT/target/release/SonicBoom" "$destination"; then
    [ "$upgrade" = 1 ] || fail "Refusing to overwrite $destination; use install.sh --upgrade to replace it"
fi
# Stage on the destination filesystem so rename is atomic. Never expose a partial binary.
staged=$(mktemp "$PREFIX/bin/.sonicboom.XXXXXXXX")
trap 'rm -f "$staged"' EXIT
trap 'exit 1' HUP INT TERM
install -m 700 "$ROOT/target/release/SonicBoom" "$staged"
cmp -s "$ROOT/target/release/SonicBoom" "$staged" || fail 'Staged binary verification failed'
# Check the staged executable and runtime before replacing a working installation.
unset LD_LIBRARY_PATH
validate_ort
"$staged" --onnx-self-test
mv -f "$staged" "$destination"
printf 'Installed %s\nSet ORT_DYLIB_PATH=%s when running independently.\n' "$destination" "$ORT_DYLIB_PATH"
