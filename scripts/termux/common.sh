# Shared by the Termux entrypoints; no host toolchain settings are changed.
set -eu
: "${PREFIX:?Run inside Termux with PREFIX set}"
export PATH="$PREFIX/bin:$PATH"
fail() { echo "SonicBoom: $*" >&2; exit 1; }
[ "$(uname -m)" = aarch64 ] || fail 'Only Android ARM64 is supported'
[ "$(uname -o)" = Android ] || fail 'Native Android/Termux is required'
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
ORT_VERSION=1.24.3
ORT_SHA256=67397e4a970e75617f765d2015ceaf911917e1d822276cfb5792744e8085cbce
ORT_ARCHIVE_SHA256=$ORT_SHA256
ORT_LIBRARY_SHA256=4d2318b3849abb8862133d3068fc7e807ed8b2671cc6d83657fff2fcb9e1caad
ORT_CACHE=${XDG_CACHE_HOME:-$HOME/.cache}/sonicboom/runtime
export ORT_DYLIB_PATH=${ORT_DYLIB_PATH:-$PREFIX/lib/sonicboom/libonnxruntime.so}
validate_ort() {
    [ -f "$ORT_DYLIB_PATH" ] || fail "Missing $ORT_DYLIB_PATH; run scripts/termux/install.sh"
    readelf -h "$ORT_DYLIB_PATH" | grep -q 'Machine:.*AArch64' || fail 'ORT must be AArch64 ELF'
    readelf -h "$ORT_DYLIB_PATH" | grep -q 'Class:.*ELF64' || fail 'ORT must be ELF64'
    deps=$(readelf -d "$ORT_DYLIB_PATH")
    echo "$deps" | grep -q '\[libandroid.so\]' || fail 'ORT must be an Android/Bionic library'
    if echo "$deps" | grep -Eq 'libc.so.6|libstdc\+\+.so|ld-linux|libpthread.so|libdl.so.2'; then
        fail 'GNU/Linux runtime is incompatible with Android'
    fi
    readelf --dyn-syms --wide "$ORT_DYLIB_PATH" | grep -q 'OrtGetApiBase' || fail 'ORT C API missing'
}
prepare_ort() {
    if [ -f "$ORT_DYLIB_PATH" ]; then
        echo "$ORT_LIBRARY_SHA256  $ORT_DYLIB_PATH" | sha256sum -c - || fail 'Existing runtime differs from pinned Android runtime; move it aside or choose a different ORT_DYLIB_PATH'
        validate_ort
        return
    fi
    mkdir -p "$ORT_CACHE"
    archive=$ORT_CACHE/onnxruntime-android-$ORT_VERSION.aar
    if [ ! -f "$archive" ]; then
        curl --fail --location --retry 3 --proto '=https' \
          "https://repo.maven.apache.org/maven2/com/microsoft/onnxruntime/onnxruntime-android/$ORT_VERSION/onnxruntime-android-$ORT_VERSION.aar" \
          -o "$archive.part"
        echo "$ORT_ARCHIVE_SHA256  $archive.part" | sha256sum -c - || fail 'ORT archive integrity check failed'
        mv "$archive.part" "$archive"
    fi
    echo "$ORT_ARCHIVE_SHA256  $archive" | sha256sum -c - || fail 'ORT cache integrity check failed; remove the archive and retry'
    staging=$(mktemp -d)
    unzip -p "$archive" jni/arm64-v8a/libonnxruntime.so > "$staging/libonnxruntime.so"
    destination=$ORT_DYLIB_PATH
    ORT_DYLIB_PATH=$staging/libonnxruntime.so validate_ort
    mkdir -p "$(dirname "$destination")"
    if [ -e "$destination" ] && ! cmp -s "$staging/libonnxruntime.so" "$destination"; then
        rm -rf "$staging"
        fail "Refusing to replace a different runtime at $destination"
    fi
    if [ ! -e "$destination" ]; then
        install -m 700 "$staging/libonnxruntime.so" "$destination.tmp"
        mv "$destination.tmp" "$destination"
    fi
    rm -rf "$staging"
    validate_ort
}
