# Source this: sets the variables cargo-apk and the NDK need (lands/docs/android.md, "Toolchain").
#   AURELIA_ANDROID_SDK  folder holding sdk/ (platforms, build-tools, ndk/<ver>, platform-tools, emulator) and
#                        jdk/ (a JDK or JRE 17: apksigner is a Java program). Default: $HOME/android-sdk.
AURELIA_ANDROID_SDK="${AURELIA_ANDROID_SDK:-$HOME/android-sdk}"
export ANDROID_HOME="$AURELIA_ANDROID_SDK/sdk"
export ANDROID_SDK_ROOT="$ANDROID_HOME"
export ANDROID_NDK_ROOT="$ANDROID_HOME/ndk/27.3.13750724"
export ANDROID_NDK_HOME="$ANDROID_NDK_ROOT"
JAVA_HOME="$(ls -d "$AURELIA_ANDROID_SDK"/jdk/jdk-17* 2>/dev/null | head -1)"
export JAVA_HOME
export PATH="$HOME/.cargo/bin:$JAVA_HOME/bin:$ANDROID_HOME/platform-tools:$ANDROID_HOME/build-tools/36.0.0:$PATH"
