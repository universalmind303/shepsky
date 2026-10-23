package dev.shepsky.core

import android.util.Log
import java.util.concurrent.atomic.AtomicLong
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.decodeFromJsonElement
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonArray

sealed interface ConnState {
    data object Idle : ConnState
    data object Connecting : ConnState
    /** Authenticated: the bridge said `hello`. */
    data object Connected : ConnState
    data class Disconnected(val detail: String?) : ConnState
    /** The bridge rejected the token. Final until the user re-pairs. */
    data object TokenRejected : ConnState
}

/** How a write (prompt, keys, focus) turned out, matched by [opId]. */
sealed interface OpResult {
    val opId: String

    data class Ok(override val opId: String) : OpResult

    data class Failed(override val opId: String, val code: String, val message: String) : OpResult {
        /** What to tell the user. */
        val friendly: String get() = UserText.opError(code, message)
    }
}

/**
 * Single source of truth for the UI: wraps the native core, parses its event
 * stream into flows, and encodes commands.
 *
 * Owned by [dev.shepsky.service.ShepskyService]; composables only read flows and
 * call the command helpers.
 */
object ShepskyRepo {
    private const val TAG = "shepsky"
    private val json = Json { ignoreUnknownKeys = true }

    @Volatile
    private var handle: Long = 0L

    private val _conn = MutableStateFlow<ConnState>(ConnState.Idle)
    val conn: StateFlow<ConnState> = _conn.asStateFlow()

    private val _state = MutableStateFlow(StateView())
    val state: StateFlow<StateView> = _state.asStateFlow()

    private val _herdrTheme = MutableStateFlow<HerdrThemeView?>(null)

    /** herdr's theme, from the bridge's hello (null until the first one). */
    val herdrTheme: StateFlow<HerdrThemeView?> = _herdrTheme.asStateFlow()

    /** Notifications (needs_input / finished) as they happen. */
    val notes = MutableSharedFlow<NoteView>(extraBufferCapacity = 16)

    private val _opResults = MutableSharedFlow<OpResult>(extraBufferCapacity = 32)

    /** One result per op id returned by [prompt], [sendKeys] and [focusAgent]. */
    val opResults: SharedFlow<OpResult> = _opResults.asSharedFlow()

    private val nextOp = AtomicLong()

    /** Ops sent but not yet acked or failed. */
    private val pending = HashSet<String>()

    private val frames = HashMap<String, MutableStateFlow<Frame?>>()

    /**
     * Panes being watched, by number of screens watching them. The bridge
     * keeps one watch per connection per pane (an unwatch ends it) and forgets
     * them all when the connection drops, so we count here and re-send.
     */
    private val watchCounts = HashMap<String, Int>()

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
        handle = ShepskyCore.nativeStart(config.toString().toByteArray(), listener)
        if (handle == 0L) {
            _conn.value = ConnState.Disconnected("native core failed to start")
        }
    }

    @Synchronized
    fun stop() {
        if (handle != 0L) {
            ShepskyCore.nativeStop(handle)
            handle = 0L
        }
        _conn.value = ConnState.Idle
        _state.value = StateView()
        failPending("not_connected")
        synchronized(frames) { frames.values.forEach { it.value = null } }
    }

    // --- commands ----------------------------------------------------------

    /** Start terminal frames for a pane; pair every call with [unwatchPane]. */
    fun watchPane(paneId: String) {
        val first = synchronized(watchCounts) {
            val n = (watchCounts[paneId] ?: 0) + 1
            watchCounts[paneId] = n
            n == 1
        }
        if (first) sendWatch(paneId)
    }

    fun unwatchPane(paneId: String) {
        val last = synchronized(watchCounts) {
            val n = (watchCounts[paneId] ?: return) - 1
            if (n == 0) watchCounts.remove(paneId) else watchCounts[paneId] = n
            n == 0
        }
        if (last) send(buildJsonObject {
            put("type", "unwatch_pane")
            put("pane_id", paneId)
        }.toString())
    }

    private fun sendWatch(paneId: String) = send(buildJsonObject {
        put("type", "watch_pane")
        put("pane_id", paneId)
    }.toString())

    /** Submit a prompt; returns its op id (see [opResults]). */
    fun prompt(paneId: String, text: String): String = sendOp { opId ->
        buildJsonObject {
            put("type", "prompt")
            put("pane_id", paneId)
            put("text", text)
            put("op_id", opId)
        }
    }

    fun sendKeys(paneId: String, keys: List<String>): String = sendOp { opId ->
        buildJsonObject {
            put("type", "send_keys")
            put("pane_id", paneId)
            putJsonArray("keys") { keys.forEach { add(JsonPrimitive(it)) } }
            put("op_id", opId)
        }
    }

    fun focusAgent(paneId: String): String = sendOp { opId ->
        buildJsonObject {
            put("type", "focus_agent")
            put("pane_id", paneId)
            put("op_id", opId)
        }
    }

    private fun sendOp(build: (String) -> JsonObject): String {
        val opId = "op${nextOp.incrementAndGet()}"
        synchronized(pending) { pending += opId }
        if (!send(build(opId).toString())) {
            resolve(OpResult.Failed(opId, "not_connected", "core not running"))
        }
        return opId
    }

    /** False when there's no native session to hand the message to. */
    private fun send(msg: String): Boolean {
        val h = handle
        if (h == 0L) return false
        ShepskyCore.nativeSend(h, msg.toByteArray())
        return true
    }

    /** Emit a result for an op we're still waiting on (each op resolves once). */
    private fun resolve(result: OpResult) {
        val wasPending = synchronized(pending) { pending.remove(result.opId) }
        if (wasPending) _opResults.tryEmit(result)
    }

    private fun failPending(code: String) {
        val ops = synchronized(pending) { pending.toList().also { pending.clear() } }
        ops.forEach { _opResults.tryEmit(OpResult.Failed(it, code, "")) }
    }

    // --- event stream ------------------------------------------------------

    private val listener = object : ShepskyCore.EventListener {
        override fun onEvent(json: ByteArray) = handleEvent(String(json))
    }

    private fun handleEvent(text: String) {
        try {
            val obj = json.parseToJsonElement(text).jsonObject
            when (obj["type"]?.jsonPrimitive?.content) {
                "conn" -> {
                    val phase = obj["phase"]?.jsonPrimitive?.content
                    val detail = obj["detail"]?.jsonPrimitive?.contentOrNull
                    if (phase == "disconnected" || phase == "unauthorized") {
                        Log.i(TAG, "link down: $detail")
                        // Anything in flight may or may not have landed.
                        if (_conn.value == ConnState.Connected) failPending("connection_lost")
                    }
                    _conn.value = when (phase) {
                        // The socket is open but auth isn't confirmed until hello.
                        "connecting", "connected" -> ConnState.Connecting
                        "unauthorized" -> ConnState.TokenRejected
                        else -> ConnState.Disconnected(detail)
                    }
                }
                "hello" -> {
                    obj["herdr_theme"]?.let { _herdrTheme.value = json.decodeFromJsonElement<HerdrThemeView>(it) }
                    _conn.value = ConnState.Connected
                    // A fresh connection has no watches yet.
                    synchronized(watchCounts) { watchCounts.keys.toList() }.forEach(::sendWatch)
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
                "ack" -> {
                    val opId = obj["op_id"]?.jsonPrimitive?.contentOrNull ?: return
                    resolve(OpResult.Ok(opId))
                }
                "error" -> {
                    val code = obj["code"]?.jsonPrimitive?.contentOrNull ?: "error"
                    val message = obj["message"]?.jsonPrimitive?.contentOrNull ?: ""
                    val opId = obj["op_id"]?.jsonPrimitive?.contentOrNull
                    if (opId != null) {
                        resolve(OpResult.Failed(opId, code, message))
                    } else {
                        // Watch errors etc.: nothing the user can act on.
                        Log.w(TAG, "bridge error $code: $message")
                    }
                }
                "pong" -> {}
                else -> {} // forward-compat: ignore unknown events
            }
        } catch (err: Exception) {
            Log.w(TAG, "bad event: ${text.take(200)}", err)
        }
    }
}
