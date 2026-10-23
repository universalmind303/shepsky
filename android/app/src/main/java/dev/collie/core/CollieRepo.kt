package dev.collie.core

import android.util.Log
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.decodeFromJsonElement
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonArray

sealed interface ConnState {
    data object Idle : ConnState
    data object Connecting : ConnState
    data object Connected : ConnState
    data class Disconnected(val detail: String?) : ConnState
}

/**
 * Single source of truth for the UI: wraps the native core, parses its event
 * stream into flows, and encodes commands.
 *
 * Owned by [dev.collie.service.CollieService]; composables only read flows and
 * call the command helpers.
 */
object CollieRepo {
    private const val TAG = "collie"
    private val json = Json { ignoreUnknownKeys = true }

    @Volatile
    private var handle: Long = 0L

    private val _conn = MutableStateFlow<ConnState>(ConnState.Idle)
    val conn: StateFlow<ConnState> = _conn.asStateFlow()

    private val _state = MutableStateFlow(StateView())
    val state: StateFlow<StateView> = _state.asStateFlow()

    /** Notifications (needs_input / finished) as they happen. */
    val notes = MutableSharedFlow<NoteView>(extraBufferCapacity = 16)

    /** Human-readable op failures, for snackbars. */
    val opErrors = MutableSharedFlow<String>(extraBufferCapacity = 16)

    private val frames = HashMap<String, MutableStateFlow<Frame?>>()

    private fun mutableFrame(paneId: String): MutableStateFlow<Frame?> = synchronized(frames) {
        frames.getOrPut(paneId) { MutableStateFlow(null) }
    }

    fun frameFlow(paneId: String): StateFlow<Frame?> = mutableFrame(paneId)

    val running: Boolean get() = handle != 0L

    @Synchronized
    fun start(url: String, token: String) {
        stop()
        _conn.value = ConnState.Connecting
        val config = buildJsonObject {
            put("url", url)
            put("token", token)
        }
        handle = CollieCore.nativeStart(config.toString().toByteArray(), listener)
        if (handle == 0L) {
            _conn.value = ConnState.Disconnected("native core failed to start")
        }
    }

    @Synchronized
    fun stop() {
        if (handle != 0L) {
            CollieCore.nativeStop(handle)
            handle = 0L
        }
        _conn.value = ConnState.Idle
        _state.value = StateView()
        synchronized(frames) { frames.values.forEach { it.value = null } }
    }

    // --- commands ----------------------------------------------------------

    fun watchPane(paneId: String) = send(buildJsonObject {
        put("type", "watch_pane")
        put("pane_id", paneId)
    }.toString())

    fun unwatchPane(paneId: String) = send(buildJsonObject {
        put("type", "unwatch_pane")
        put("pane_id", paneId)
    }.toString())

    fun prompt(paneId: String, text: String) = send(buildJsonObject {
        put("type", "prompt")
        put("pane_id", paneId)
        put("text", text)
    }.toString())

    fun sendKeys(paneId: String, keys: List<String>) = send(buildJsonObject {
        put("type", "send_keys")
        put("pane_id", paneId)
        putJsonArray("keys") { keys.forEach { add(JsonPrimitive(it)) } }
    }.toString())

    fun focusAgent(paneId: String) = send(buildJsonObject {
        put("type", "focus_agent")
        put("pane_id", paneId)
    }.toString())

    private fun send(msg: String) {
        val h = handle
        if (h != 0L) CollieCore.nativeSend(h, msg.toByteArray())
    }

    // --- event stream ------------------------------------------------------

    private val listener = object : CollieCore.EventListener {
        override fun onEvent(json: ByteArray) = handleEvent(String(json))
    }

    private fun handleEvent(text: String) {
        try {
            val obj = json.parseToJsonElement(text).jsonObject
            when (obj["type"]?.jsonPrimitive?.content) {
                "conn" -> {
                    val phase = obj["phase"]?.jsonPrimitive?.content
                    val detail = obj["detail"]?.jsonPrimitive?.takeIf { it.isString }?.content
                    _conn.value = when (phase) {
                        "connecting" -> ConnState.Connecting
                        "connected" -> ConnState.Connected
                        else -> ConnState.Disconnected(detail)
                    }
                }
                "state" -> {
                    val state = obj["state"] ?: return
                    _state.value = json.decodeFromJsonElement<StateView>(state)
                }
                "notify" -> {
                    val note = obj["note"] ?: return
                    notes.tryEmit(json.decodeFromJsonElement<NoteView>(note))
                }
                "terminal_frame" -> {
                    val paneId = obj["pane_id"]?.jsonPrimitive?.content ?: return
                    val frame = obj["frame"] ?: return
                    mutableFrame(paneId).value = json.decodeFromJsonElement<Frame>(frame)
                }
                "error" -> {
                    val code = obj["code"]?.jsonPrimitive?.content ?: "error"
                    val message = obj["message"]?.jsonPrimitive?.content ?: ""
                    // Auth failure means the pairing is wrong — surface it on
                    // the connection state, not as a passing snackbar.
                    if (code == "unauthorized") {
                        _conn.value = ConnState.Disconnected("unauthorized — check the token")
                    } else {
                        opErrors.tryEmit("$code: $message")
                    }
                }
                "hello", "ack", "pong" -> {} // nothing to do (yet)
                else -> {} // forward-compat: ignore unknown events
            }
        } catch (err: Exception) {
            Log.w(TAG, "bad event: ${text.take(200)}", err)
        }
    }
}
