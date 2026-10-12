#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
root="$PWD"
sdk="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-$root/.android-tools/sdk}}"
ndk="${ANDROID_NDK_HOME:-$sdk/ndk/28.2.13676358}"
case "$(uname -s)" in
  Linux*) host=linux-x86_64 ;;
  Darwin*) host=darwin-x86_64 ;;
  *) echo "Build with Android Studio on Linux/macOS, or use WSL2 on Windows." >&2; exit 1 ;;
esac
toolchain="$ndk/toolchains/llvm/prebuilt/$host/bin"
if [ ! -x "$toolchain/llvm-ar" ]; then
  echo "Install NDK 28.2.13676358 or set ANDROID_NDK_HOME." >&2
  exit 1
fi
for abi in ${QRTX_ANDROID_ABIS:-arm64-v8a x86_64}; do
  case "$abi" in
    arm64-v8a) target=aarch64-linux-android ;;
    x86_64) target=x86_64-linux-android ;;
    *) echo "Unsupported ABI: $abi" >&2; exit 1 ;;
  esac
  export "CC_${target//-/_}=$toolchain/${target}26-clang"
  export "AR_${target//-/_}=$toolchain/llvm-ar"
  cargo_key="${target^^}"
  cargo_key="${cargo_key//-/_}"
  export "CARGO_TARGET_${cargo_key}_LINKER=$toolchain/${target}26-clang"
  export "CARGO_TARGET_${cargo_key}_RUSTFLAGS=-C link-arg=-Wl,-z,max-page-size=16384"
  cargo build --locked --release -p qrtx-android --target "$target"
  mkdir -p "android/app/src/main/jniLibs/$abi"
  cp "target/$target/release/libqrtx_android.so" "android/app/src/main/jniLibs/$abi/"
done
