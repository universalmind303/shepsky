package dev.collie.core

/**
 * The whole JNI surface: three functions and one callback. Every payload is
 * UTF-8 JSON as a ByteArray (see crates/collie-android/src/lib.rs).
 */
object CollieCore {
    init {
        System.loadLibrary("collie_android")
    }

    interface EventListener {
        /** Called in order, from one dedicated native thread. */
        fun onEvent(json: ByteArray)
    }

    external fun nativeStart(config: ByteArray, listener: EventListener): Long
    external fun nativeSend(handle: Long, msg: ByteArray)
    external fun nativeStop(handle: Long)
}
