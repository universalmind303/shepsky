package dev.shepsky.core

/**
 * The whole JNI surface: three functions and one callback. Every payload is
 * UTF-8 JSON as a ByteArray (see crates/shepsky-android/src/lib.rs).
 */
object ShepskyCore {
    init {
        System.loadLibrary("shepsky_android")
    }

    interface EventListener {
        /** Called in order, from one dedicated native thread. */
        fun onEvent(json: ByteArray)
    }

    external fun nativeStart(config: ByteArray, listener: EventListener): Long
    external fun nativeSend(handle: Long, msg: ByteArray)
    external fun nativeStop(handle: Long)
}
