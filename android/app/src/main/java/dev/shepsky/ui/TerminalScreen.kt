package dev.shepsky.ui

import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.calculateZoom
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.sp
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.imePadding
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import dev.shepsky.core.ShepskyRepo
import dev.shepsky.data.Prefs
import dev.shepsky.core.ConnState
import dev.shepsky.core.Frame

@Composable
fun TerminalScreen(paneId: String, onOpenSettings: () -> Unit, onBack: () -> Unit) {
    DisposableEffect(paneId) {
        ShepskyRepo.watchPane(paneId)
        onDispose { ShepskyRepo.unwatchPane(paneId) }
    }
    val frame by ShepskyRepo.frameFlow(paneId).collectAsState()
    val state by ShepskyRepo.state.collectAsState()
    val conn by ShepskyRepo.conn.collectAsState()
    val agent = state.agents.firstOrNull { it.pane_id == paneId }
    val t = LocalTerm.current
    val snackbar = remember { SnackbarHostState() }
    val writes = rememberAgentWrites(paneId, snackbar)
    val connected = conn == ConnState.Connected
    // Tapping the terminal (say, the agent's own input box) means "let me
    // type": focus the prompt line and bring up the keyboard.
    val promptFocus = remember { FocusRequester() }
    val keyboard = LocalSoftwareKeyboardController.current

    Scaffold(
        topBar = {
            TopLine(
                segments(
                    Seg(agent?.label ?: "terminal", bold = true),
                    *listOfNotNull(
                        agent?.workspace_label?.let { Seg(it, t.dim) },
                        agent?.let { Seg("[${statusGlyph(it.status)}] ${statusLabel(it.status)}", statusColor(it.status)) },
                    ).toTypedArray(),
                ),
                onBack = onBack,
            )
        },
        bottomBar = {
            Column(Modifier.fillMaxWidth().navigationBarsPadding().imePadding()) {
                Rule()
                PromptLine(writes, connected = connected, blocked = agent?.status == "blocked", focus = promptFocus)
                KeyBar(enabled = connected, onKey = writes::key)
            }
        },
        snackbarHost = { TermSnackbarHost(snackbar) },
    ) { padding ->
        Column(Modifier.fillMaxSize().padding(padding)) {
            ConnBanner(onOpenSettings)
            TerminalView(frame) {
                promptFocus.requestFocus()
                keyboard?.show()
            }
        }
    }
}

@Composable
private fun TerminalView(frame: Frame?, onTap: () -> Unit) {
    val context = LocalContext.current
    val vScroll = rememberScrollState()
    val hScroll = rememberScrollState()
    // Pinch to zoom: real agents draw 130+ columns, which no single size
    // makes readable on a phone. Remembered across sessions.
    var fontSize by remember { mutableFloatStateOf(Prefs.terminalFontSize(context)) }

    // Stick to the bottom as new frames land.
    LaunchedEffect(frame) { vScroll.scrollTo(vScroll.maxValue) }

    Box(
        modifier = Modifier
            .fillMaxSize()
            .clickable(interactionSource = remember { MutableInteractionSource() }, indication = null, onClick = onTap)
            .pinchToZoom { zoom ->
                fontSize = (fontSize * zoom).coerceIn(MIN_TERMINAL_SP, MAX_TERMINAL_SP)
                Prefs.setTerminalFontSize(context, fontSize)
            }
            .verticalScroll(vScroll)
            .horizontalScroll(hScroll)
            .padding(10.dp),
    ) {
        val current = frame
        if (current == null) {
            BlinkingLine("waiting for frames")
        } else {
            TerminalLines(current.lines, fontSize = fontSize.sp)
        }
    }
}

private const val MIN_TERMINAL_SP = 7f
private const val MAX_TERMINAL_SP = 22f

/**
 * Two-finger pinch reports zoom factors; one finger is left alone for the
 * scrolls underneath. Watches the Initial pass so a pinch doesn't also
 * scroll.
 */
private fun Modifier.pinchToZoom(onZoom: (Float) -> Unit) = pointerInput(Unit) {
    awaitEachGesture {
        awaitFirstDown(requireUnconsumed = false, pass = PointerEventPass.Initial)
        do {
            val event = awaitPointerEvent(PointerEventPass.Initial)
            if (event.changes.count { it.pressed } >= 2) {
                val zoom = event.calculateZoom()
                if (zoom != 1f) onZoom(zoom)
                event.changes.forEach { it.consume() }
            }
        } while (event.changes.any { it.pressed })
    }
}

@Composable
private fun KeyBar(enabled: Boolean, onKey: (String) -> Unit) {
    Column(Modifier.fillMaxWidth()) {
        Rule()
        // Every key visible: they share the width (~50dp each on a phone).
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 12.dp, vertical = 8.dp),
            horizontalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            listOf(
                "esc" to "esc",
                "tab" to "tab",
                "↑" to "up",
                "↓" to "down",
                "←" to "left",
                "→" to "right",
                "⏎" to "enter",
            ).forEach { (label, key) ->
                KeyCap(label, onClick = { onKey(key) }, enabled = enabled, modifier = Modifier.weight(1f))
            }
        }
    }
}
