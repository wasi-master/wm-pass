# The core's native library calls back by name.
-keep class com.wasimaster.wmpass.core.Native { *; }
-keep class com.wasimaster.wmpass.core.NativeFailure { <init>(java.lang.String); }

# Shizuku builds the Wi-Fi reader by name in its own process.
-keep class com.wasimaster.wmpass.system.WifiReader { <init>(); }
