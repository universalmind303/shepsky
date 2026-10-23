package dev.collie.ui

import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.platform.LocalContext
import dev.collie.core.CollieRepo
import dev.collie.core.ConnState
import dev.collie.core.UserText
import dev.collie.data.Prefs

/** Shown whenever the phone ↔ Mac ↔ herdr chain isn't fully up. */
@Composable
fun ConnBanner(onOpenSettings: (() -> Unit)? = null) {
    val conn by CollieRepo.conn.collectAsState()
    val state by CollieRepo.state.collectAsState()
    val context = LocalContext.current
    val t = LocalTerm.current
    val text = UserText.connection(conn, Prefs.url(context))
        ?: if (!state.herdr_connected) "Your Mac's bridge is reconnecting to herdr…" else return
    // Transient states in yellow, broken ones in red.
    val transient = conn == ConnState.Connecting || conn == ConnState.Connected
    AlertLine(
        text,
        color = if (transient) t.yellow else t.red,
        action = if (conn == ConnState.TokenRejected) onOpenSettings?.let { "re-pair" to it } else null,
    )
}

/** `● linked`, for the status line. */
@Composable
fun linkSeg(): Seg {
    val conn by CollieRepo.conn.collectAsState()
    val state by CollieRepo.state.collectAsState()
    val t = LocalTerm.current
    return when (conn) {
        ConnState.Connected ->
            if (state.herdr_connected) Seg("● linked", t.green) else Seg("◐ herdr…", t.yellow)
        ConnState.Connecting -> Seg("◐ linking", t.yellow)
        ConnState.Idle -> Seg("○ unpaired", t.dim)
        ConnState.TokenRejected -> Seg("× rejected", t.red)
        is ConnState.Disconnected -> Seg("○ offline", t.red)
    }
}
