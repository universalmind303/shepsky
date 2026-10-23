package dev.collie.core

import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonPrimitive

/**
 * Kotlin mirrors of the bridge protocol (docs/PROTOCOL.md). Property names
 * match the wire exactly; parsing ignores unknown fields. The Rust source of
 * truth is crates/collie-core/src/protocol.rs — keep PROTO_VERSION in sync.
 */
const val PROTO_VERSION = 2

/** herdr's theme from `hello` (`dark`/`light` set when herdr auto-switches). */
@Serializable
data class HerdrThemeView(
    val name: String,
    val dark: String? = null,
    val light: String? = null,
) {
    fun pick(systemDark: Boolean): String =
        if (dark != null && light != null) (if (systemDark) dark else light) else name
}

@Serializable
data class StateView(
    val herdr_version: String = "",
    val herdr_connected: Boolean = false,
    val workspaces: List<WorkspaceView> = emptyList(),
    val agents: List<AgentView> = emptyList(),
)

@Serializable
data class WorkspaceView(
    val workspace_id: String,
    val label: String? = null,
    val number: Long? = null,
    val status: String? = null,
    val focused: Boolean = false,
)

@Serializable
data class AgentView(
    val pane_id: String,
    val terminal_id: String = "",
    val status: String = "unknown",
    val label: String = "agent",
    val kind: String? = null,
    val title: String? = null,
    val workspace_id: String = "",
    val workspace_label: String? = null,
    val cwd: String? = null,
    val focused: Boolean = false,
)

@Serializable
data class NoteView(
    val kind: String,
    val pane_id: String,
    val agent_label: String,
    val title: String? = null,
    val workspace_label: String? = null,
)

@Serializable
data class Frame(
    val cols: Int = 0,
    val rows: Int = 0,
    val lines: List<List<Span>> = emptyList(),
)

/**
 * A run of styled terminal text. [fg]/[bg] are a number (ANSI 0–15, drawn
 * in the phone's theme) or a `#rrggbb` string; absent = the theme default.
 * [r] = reverse video.
 */
@Serializable
data class Span(
    val t: String,
    val fg: JsonPrimitive? = null,
    val bg: JsonPrimitive? = null,
    val b: Boolean = false,
    val i: Boolean = false,
    val u: Boolean = false,
    val r: Boolean = false,
)
