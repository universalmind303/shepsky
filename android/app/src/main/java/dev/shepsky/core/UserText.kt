package dev.shepsky.core

/**
 * Plain-language copy for failures. Raw codes and socket errors go to logcat;
 * the user sees what happened and what to do about it.
 */
object UserText {
    fun opError(code: String, message: String): String = when (code) {
        "agent_blocked" -> "It's waiting on a question — answer it with the buttons first."
        "agent_not_ready" -> "That agent isn't running in its pane right now. Nothing was sent."
        "agent_not_found", "not_found" -> "That agent is gone."
        "not_connected" -> "Not connected to your Mac — reconnecting…"
        "connection_lost" -> "Lost the connection before your Mac confirmed. Check the terminal before sending again."
        "too_large" -> "That prompt is too long to send."
        else -> "Couldn't send: ${message.ifBlank { code }}"
    }

    /** Banner / status text for a connection that isn't up, or null when it is. */
    fun connection(conn: ConnState, url: String): String? = when (conn) {
        ConnState.Connected -> null
        ConnState.Idle -> "Not connected — open Settings to pair."
        ConnState.Connecting -> "Connecting to your Mac…"
        ConnState.TokenRejected -> "Your Mac rejected the token. Pair again in Settings."
        is ConnState.Disconnected -> {
            val detail = conn.detail.orEmpty().lowercase()
            when {
                "refused" in detail ->
                    "Can't reach the bridge at $url — is shepsky-bridge running?"
                listOf("timed out", "unreachable", "no route", "failed to lookup", "dns").any { it in detail } ->
                    "Can't reach $url — check your network or Tailscale."
                else -> "Lost the connection to your Mac — reconnecting…"
            }
        }
    }
}
