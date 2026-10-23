package dev.shepsky.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.calculateZoom
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.isImeVisible
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.SnackbarHostState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.shepsky.core.ConnState
import dev.shepsky.core.Frame
import dev.shepsky.core.ShepskyRepo
import dev.shepsky.data.Prefs
import kotlinx.coroutines.delay

/** Quick replies stay dead this long after the agent starts asking, so a tap
 *  that was already on its way can't approve a question nobody has read. */
private const val ARM_DELAY_MS = 600L

private const val MIN_TERMINAL_SP = 7f
private const val MAX_TERMINAL_SP = 22f

/**
 * One agent: its conversation (or its live terminal), a prompt line, and
 * the keys. Opening it is one tap from the overview or a notification;
 * while it waits on you, its question is at the bottom of the chat.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun AgentScreen(paneId: String, onOpenSettings: () -> Unit, onBack: () -> Unit) {
    val state by ShepskyRepo.state.collectAsState()
    val conn by ShepskyRepo.conn.collectAsState()
    val frame by ShepskyRepo.frameFlow(paneId).collectAsState()
    val chat by ShepskyRepo.chatFlow(paneId).collectAsState()
    val agent = state.agents.firstOrNull { it.pane_id == paneId }
    val connected = conn == ConnState.Connected
    val blocked = agent?.status == "blocked"
    val t = LocalTerm.current

    val snackbar = remember { SnackbarHostState() }
    val writes = rememberAgentWrites(paneId, snackbar)
    // Tapping the terminal (say, the agent's own input box) means "let me
    // type": focus the prompt line and bring up the keyboard.
    val promptFocus = remember { FocusRequester() }
    val keyboard = LocalSoftwareKeyboardController.current

    var armed by remember { mutableStateOf(false) }
    LaunchedEffect(blocked) {
        armed = false
        if (blocked) {
            delay(ARM_DELAY_MS)
            armed = true
        }
    }

    DisposableEffect(paneId) {
        ShepskyRepo.watchPane(paneId)
        ShepskyRepo.watchChat(paneId)
        onDispose {
            ShepskyRepo.unwatchPane(paneId)
            ShepskyRepo.unwatchChat(paneId)
        }
    }
    // Chat unless there's no conversation to show, until you pick a tab.
    var picked by rememberSaveable(paneId) { mutableStateOf<String?>(null) }
    val tab = picked ?: if (chat.unavailable != null && chat.items.isEmpty()) TERMINAL else CHAT
    // Looking at a finished agent clears "finished". Only then: agent.focus
    // also switches herdr on the Mac to this agent's tab.
    LaunchedEffect(paneId, connected, agent?.status) {
        if (connected && agent?.status == "done") ShepskyRepo.focusAgent(paneId)
    }

    Scaffold(
        topBar = {
            TopLine(
                segments(
                    Seg(agent?.label ?: "agent", bold = true),
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
                PromptLine(writes, connected = connected, blocked = blocked, focus = promptFocus)
                // While typing, the keys step aside so the keyboard doesn't
                // leave the terminal a sliver.
                if (!WindowInsets.isImeVisible) {
                    Rule()
                    Keys(
                        blocked = blocked,
                        armed = armed && connected,
                        connected = connected,
                        codex = agent?.kind == "codex",
                        onKey = writes::key,
                    )
                }
            }
        },
        snackbarHost = { TermSnackbarHost(snackbar) },
    ) { padding ->
        Column(Modifier.fillMaxSize().padding(padding)) {
            ConnBanner(onOpenSettings)
            if (agent != null) Tabs(tab, onPick = { picked = it })
            when {
                agent != null && tab == CHAT -> ChatView(
                    chat = chat,
                    blocked = blocked,
                    frame = frame,
                    onLoadOlder = { ShepskyRepo.loadOlderChat(paneId) },
                    onShowTerminal = { picked = TERMINAL },
                )
                agent != null -> TerminalView(frame) {
                    promptFocus.requestFocus()
                    keyboard?.show()
                }
                connected && state.herdr_connected -> Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                    BlinkingLine("this agent is gone")
                }
                else -> Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                    BlinkingLine("waiting for your mac")
                }
            }
        }
    }
}

private const val CHAT = "chat"
private const val TERMINAL = "terminal"

/** `chat  terminal`, the current one underlined in the accent. */
@Composable
private fun Tabs(tab: String, onPick: (String) -> Unit) {
    val t = LocalTerm.current
    Column {
        Row(Modifier.fillMaxWidth().padding(horizontal = 4.dp)) {
            listOf(CHAT, TERMINAL).forEach { name ->
                val on = name == tab
                Column(
                    Modifier
                        .heightIn(min = 44.dp)
                        .clickable { onPick(name) }
                        .padding(horizontal = 12.dp),
                    verticalArrangement = Arrangement.Center,
                    horizontalAlignment = Alignment.CenterHorizontally,
                ) {
                    Text(
                        name,
                        color = if (on) t.accent else t.dim,
                        fontWeight = if (on) FontWeight.Bold else null,
                        style = MaterialTheme.typography.bodyMedium,
                    )
                    Box(Modifier.padding(top = 4.dp).width(32.dp).height(2.dp).background(if (on) t.accent else t.bg))
                }
            }
        }
        Rule()
    }
}

/**
 * Two rows of keys in a fixed layout. Top row: the quick replies, lit up in
 * red while the agent is asking; 1/2/3 only work then (after the arming
 * delay), so nothing shifts or fires under a finger as the status changes.
 * Codex gets shift+left too.
 */
@Composable
private fun Keys(blocked: Boolean, armed: Boolean, connected: Boolean, codex: Boolean, onKey: (String) -> Unit) {
    Column(
        Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 8.dp),
        verticalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            listOf("1" to "1", "2" to "2", "3" to "3", "esc" to "esc", "⏎" to "enter").forEach { (label, key) ->
                val digit = key.length == 1
                KeyCap(
                    label,
                    onClick = { onKey(key) },
                    enabled = if (blocked) armed else connected && !digit,
                    hot = blocked,
                    contentDescription = if (key == "enter") "Enter" else null,
                    modifier = Modifier.weight(1f),
                )
            }
        }
        Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            val navigation = listOf("tab" to "tab", "↑" to "up", "↓" to "down", "←" to "left", "→" to "right")
            (navigation + if (codex) listOf("⇧←" to "shift+left") else emptyList()).forEach { (label, key) ->
                KeyCap(
                    label,
                    onClick = { onKey(key) },
                    enabled = connected,
                    contentDescription = if (key == "shift+left") "Shift left" else null,
                    modifier = Modifier.weight(1f),
                )
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

    // Follow new output only while you're at the bottom: scrolling up
    // pauses it so you can read, [↓ latest] resumes. Decided when a scroll
    // gesture ends, never by a frame arriving.
    var following by remember { mutableStateOf(true) }
    val slack = with(LocalDensity.current) { 32.dp.toPx() }
    LaunchedEffect(vScroll) {
        var wasScrolling = false
        snapshotFlow { vScroll.isScrollInProgress }.collect { scrolling ->
            if (wasScrolling && !scrolling) following = vScroll.value >= vScroll.maxValue - slack
            wasScrolling = scrolling
        }
    }
    LaunchedEffect(frame, following, fontSize) {
        if (following) vScroll.scrollTo(vScroll.maxValue)
    }

    Box(Modifier.fillMaxSize()) {
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
            if (frame == null) {
                BlinkingLine("waiting for frames")
            } else {
                TerminalLines(frame.lines, fontSize = fontSize.sp)
            }
        }
        if (!following) {
            KeyCap(
                "↓ latest",
                onClick = { following = true },
                modifier = Modifier
                    .align(Alignment.BottomEnd)
                    .padding(12.dp)
                    .background(LocalTerm.current.bg),
            )
        }
    }
}

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
