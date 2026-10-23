# The Rust core (libshepsky_android.so) calls back into Kotlin over JNI by
# name: EventListener.onEvent([B)V on whatever listener it was handed. R8
# can't see native callers, so without these it renames or drops onEvent and
# the first event crashes the app. (The native methods themselves are kept by
# the default rules.)
-keep interface dev.shepsky.core.ShepskyCore$EventListener { *; }
-keepclassmembers class * implements dev.shepsky.core.ShepskyCore$EventListener {
    public void onEvent(byte[]);
}
