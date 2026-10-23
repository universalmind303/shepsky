//! JNI surface for the Android app. Deliberately tiny — three functions and
//! one callback, all payloads UTF-8 JSON as `byte[]` (sidesteps JNI's
//! modified-UTF-8). The session itself lives in `shepsky-client`, shared with
//! the iOS shim; this crate only bridges it to the JVM.
//!
//! Kotlin side: `dev.shepsky.core.ShepskyCore`
//!   external fun nativeStart(config: ByteArray, listener: EventListener): Long
//!   external fun nativeSend(handle: Long, msg: ByteArray)
//!   external fun nativeStop(handle: Long)
//! with `interface EventListener { fun onEvent(json: ByteArray) }`.
//!
//! Threading: one dedicated OS thread stays attached to the JVM and delivers
//! a session's events to the listener in order.

use std::sync::Once;

use jni::JNIEnv;
use jni::objects::{GlobalRef, JByteArray, JClass, JObject, JValue};
use jni::sys::jlong;

fn init_logging() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        android_logger::init_once(
            android_logger::Config::default()
                .with_max_level(log::LevelFilter::Info)
                .with_tag("shepsky"),
        );
    });
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_shepsky_core_ShepskyCore_nativeStart(
    env: JNIEnv,
    _class: JClass,
    config: JByteArray,
    listener: JObject,
) -> jlong {
    init_logging();

    let config = match env.convert_byte_array(&config) {
        Ok(bytes) => bytes,
        Err(err) => {
            log::error!("nativeStart: bad config array: {err}");
            return 0;
        }
    };
    let (Ok(vm), Ok(listener)) = (env.get_java_vm(), env.new_global_ref(&listener)) else {
        log::error!("nativeStart: could not grab JavaVM/listener");
        return 0;
    };
    let (handle, events) = match shepsky_client::start(&config) {
        Ok(started) => started,
        Err(err) => {
            log::error!("nativeStart: {err}");
            return 0;
        }
    };

    // Dedicated delivery thread: attached once, detached when the session's
    // event stream ends (after nativeStop).
    std::thread::Builder::new()
        .name("shepsky-events".into())
        .spawn(move || deliver_events(vm, listener, events))
        .expect("spawn event thread");

    handle as jlong
}

fn deliver_events(vm: jni::JavaVM, listener: GlobalRef, events: std::sync::mpsc::Receiver<String>) {
    let mut env = match vm.attach_current_thread() {
        Ok(env) => env,
        Err(err) => {
            log::error!("event thread could not attach to JVM: {err}");
            return;
        }
    };
    while let Ok(event) = events.recv() {
        let Ok(array) = env.byte_array_from_slice(event.as_bytes()) else {
            continue;
        };
        let result = env.call_method(
            listener.as_obj(),
            "onEvent",
            "([B)V",
            &[JValue::Object(array.as_ref())],
        );
        if result.is_err() {
            // A throwing listener must not kill delivery (or the process).
            if env.exception_check().unwrap_or(false) {
                let _ = env.exception_describe();
                let _ = env.exception_clear();
            }
        }
        // Local refs pile up in long-lived attached threads; drop eagerly.
        let _ = env.delete_local_ref(array);
    }
    log::info!("event thread exiting");
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_shepsky_core_ShepskyCore_nativeSend(
    env: JNIEnv,
    _class: JClass,
    handle: jlong,
    msg: JByteArray,
) {
    let Ok(bytes) = env.convert_byte_array(&msg) else { return };
    let Ok(text) = String::from_utf8(bytes) else { return };
    if !shepsky_client::send(handle as u64, text) {
        log::warn!("command dropped (unknown handle {handle}, or queue full)");
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_shepsky_core_ShepskyCore_nativeStop(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    shepsky_client::stop(handle as u64);
}
