#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
root="$PWD"
if [ -z "${JAVA_HOME:-}" ] && [ -d "$root/.android-tools/jdk" ]; then
  export JAVA_HOME="$root/.android-tools/jdk"
fi
export ANDROID_HOME="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-$root/.android-tools/sdk}}"
export PATH="${JAVA_HOME:+$JAVA_HOME/bin:}$PATH"
cd android
if [ "$#" -eq 0 ]; then set -- assembleDebug; fi
./gradlew "$@"
