#!/system/bin/sh
. "$(dirname "$0")/common.sh"
pkg update
pkg install -y rust clang cmake make pkg-config openssl libopus git curl unzip binutils
"$ROOT/scripts/termux/build.sh"
destination=$PREFIX/bin/sonicboom
if [ -e "$destination" ] && ! cmp -s "$ROOT/target/release/SonicBoom" "$destination"; then
    fail "Refusing to overwrite $destination; move it aside to install this build"
fi
install -m 700 "$ROOT/target/release/SonicBoom" "$destination"
printf 'Installed %s\nSet ORT_DYLIB_PATH=%s when running independently.\n' "$destination" "$ORT_DYLIB_PATH"
