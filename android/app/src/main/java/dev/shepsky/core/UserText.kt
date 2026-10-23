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

    /** Why an agent didn't start. The bridge's own messages for a bad
     *  directory or name are already written for people. */
    fun startError(code: String, message: String): String = when (code) {
        "bad_directory", "invalid_agent_name" -> message.ifBlank { "Check the directory and name." }
        "unsupported_kind" -> "Your Mac doesn't allow starting that kind of agent from the phone."
        "agent_name_taken" -> "That name is taken. Pick another, or leave it blank."
        "agent_pane_busy", "agent_pane_unavailable" -> "The new tab's shell didn't come up in time. Try again."
        "not_found" -> "That workspace is gone."
        "not_connected", "connection_lost" -> opError(code, message)
        else -> "Couldn't start the agent: ${message.ifBlank { code }}"
    }

    /** Why a rename or close didn't happen. */
    fun manageError(code: String, message: String): String = when {
        code == "agent_name_taken" -> "Another agent already has that name."
        code == "invalid_agent_name" -> "Names are lowercase letters, digits, - or _, start with a letter, up to 32."
        code == "agent_not_found" || code == "not_found" -> "That pane isn't running an agent anymore."
        "pending" in code -> "It's still starting up. Try again in a moment."
        code == "not_connected" || code == "connection_lost" -> opError(code, message)
        else -> "Didn't work: ${message.ifBlank { code }}"
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
