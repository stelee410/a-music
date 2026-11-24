#!/bin/sh
# Build the Android app: the engine for arm64 (crates/agb-android → libagb.so, and the agb program), the songs of this
# repository as .agbw (made by web/make-songs.sh), and the package.
#
#   android/build.sh            build app/build/outputs/apk/release/app-release.apk
#   android/build.sh install    … and install it on the device that is plugged in, and start it
#
# Needs: the Android SDK with an NDK (ANDROID_HOME, or ~/Library/Android/sdk), a JDK, Gradle, and
# the Rust target (rustup target add aarch64-linux-android).
set -e
cd "$(dirname "$0")/.."

SDK="${ANDROID_HOME:-$HOME/Library/Android/sdk}"
NDK="${ANDROID_NDK_HOME:-$(ls -d "$SDK"/ndk/* 2>/dev/null | sort | tail -1)}"
[ -d "$NDK" ] || { echo "no Android NDK under $SDK/ndk (install one with sdkmanager 'ndk;27.2.12479018')"; exit 1; }
BIN="$(ls -d "$NDK"/toolchains/llvm/prebuilt/*/bin | head -1)"

# The engine. API 30 is the oldest Android the app runs on.
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$BIN/aarch64-linux-android30-clang"
export CC_aarch64_linux_android="$BIN/aarch64-linux-android30-clang"
export AR_aarch64_linux_android="$BIN/llvm-ar"
cargo build --release --target aarch64-linux-android -p agb-android -p agb-cli
mkdir -p android/app/src/main/jniLibs/arm64-v8a
cp target/aarch64-linux-android/release/libagb.so android/app/src/main/jniLibs/arm64-v8a/libagb.so
# The command-line program too: the app runs it for AI performances, the way the Mac app does.
# (Android only lets an app run a program that was installed as one of its libraries.)
cp target/aarch64-linux-android/release/agb android/app/src/main/jniLibs/arm64-v8a/libagbcli.so

# The songs: the same files the web player has.
ls web/songs/*.agbw >/dev/null 2>&1 || web/make-songs.sh
rm -rf android/app/src/main/assets/songs
mkdir -p android/app/src/main/assets/songs
cp web/songs/*.agbw android/app/src/main/assets/songs/

cd android
echo "sdk.dir=$SDK" > local.properties
# The release build: Android runs a debug build without optimising it, and the mixer (which works
# on every sample, in Kotlin) then cannot keep up, which is heard as crackling.
gradle ${GRADLE_FLAGS:-} assembleRelease
ls -l app/build/outputs/apk/release/app-release.apk

if [ "$1" = "install" ]; then
  adb install -r app/build/outputs/apk/release/app-release.apk
  adb shell am start -n dev.agb.duo/.MainActivity
fi
