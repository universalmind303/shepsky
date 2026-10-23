package dev.shepsky.ui

import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.imePadding
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
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import dev.shepsky.core.ShepskyRepo
import dev.shepsky.core.ConnState
import dev.shepsky.core.Frame
import kotlinx.coroutines.delay

private const val PREVIEW_LINES = 16

/** Quick replies stay dead this long after the agent starts asking, so a tap
 *  that was already on its way can't approve a question nobody has read. */
private const val ARM_DELAY_MS = 600L

@Composable
fun AgentScreen(
    paneId: String,
    onOpenTerminal: () -> Unit,
    onOpenSettings: () -> Unit,
    onBack: () -> Unit,
) {
    val state by ShepskyRepo.state.collectAsState()
    val conn by ShepskyRepo.conn.collectAsState()
    val frame by ShepskyRepo.frameFlow(paneId).collectAsState()
    val agent = state.agents.firstOrNull { it.pane_id == paneId }
    val connected = conn == ConnState.Connected
    val blocked = agent?.status == "blocked"

    val snackbar = remember { SnackbarHostState() }
    val writes = rememberAgentWrites(paneId, snackbar)

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
        onDispose { ShepskyRepo.unwatchPane(paneId) }
    }
    // Looking at a finished agent clears "finished". Only then: agent.focus
    // also switches herdr on the Mac to this agent's tab.
    LaunchedEffect(paneId, connected, agent?.status) {
        if (connected && agent?.status == "done") ShepskyRepo.focusAgent(paneId)
    }
    val t = LocalTerm.current
    Scaffold(
        topBar = {
            TopLine(
                segments(
                    Seg(agent?.label ?: "agent", bold = true),
                    *listOfNotNull(agent?.workspace_label?.let { Seg(it, t.dim) }).toTypedArray(),
                ),
                onBack = onBack,
                actions = {
                    if (agent != null) TapText("[term]", onOpenTerminal, contentDescription = "Open terminal")
                },
            )
        },
        bottomBar = {
            if (agent != null) {
                ReplyBar(writes, blocked = blocked, armed = armed && connected, connected = connected)
            }
        },
        snackbarHost = { TermSnackbarHost(snackbar) },
    ) { padding ->
        Column(Modifier.fillMaxSize().padding(padding)) {
            ConnBanner(onOpenSettings)
            if (agent == null) {
                Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                    Text(
                        if (connected && state.herdr_connected) "this agent is gone" else "waiting for your mac…",
                        color = t.dim,
                        style = MaterialTheme.typography.bodyMedium,
                    )
                }
                return@Column
            }
            Column(
                modifier = Modifier
                    .fillMaxSize()
                    .verticalScroll(rememberScrollState())
                    .padding(horizontal = 16.dp, vertical = 12.dp),
                verticalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                Text(
                    statusTag(agent.status),
                    style = MaterialTheme.typography.bodyLarge,
                    fontWeight = FontWeight.Bold,
                )
                agent.title?.takeIf { it.isNotBlank() }?.let {
                    Text(it, style = MaterialTheme.typography.bodyMedium)
                }
                agent.cwd?.let {
                    Text(it, color = t.dim, style = MaterialTheme.typography.bodySmall)
                }
                OutputPreview(frame, onClick = onOpenTerminal, modifier = Modifier.padding(top = 10.dp))
            }
        }
    }
}

/** The last screenful of output, so nobody answers a question blind. */
@Composable
private fun OutputPreview(frame: Frame?, onClick: () -> Unit, modifier: Modifier = Modifier) {
    val t = LocalTerm.current
    Column(modifier.fillMaxWidth()) {
        SectionRule("output · tap to open")
        Column(
            Modifier
                .fillMaxWidth()
                .border(1.dp, t.dim)
                .clickable(onClick = onClick)
                .padding(10.dp),
        ) {
            val lines = frame?.lines?.dropLastWhile { it.isBlankLine() }?.takeLast(PREVIEW_LINES)
            if (lines == null) {
                Text("waiting for output…", color = t.dim, style = MaterialTheme.typography.bodySmall)
            } else {
                ScrollingTerminalLines(lines)
            }
        }
    }
}

/**
 * Quick replies over the prompt box. Fixed layout whatever the status: the
 * keys only light up while the agent is asking, so nothing shifts under a
 * finger when the status changes.
 */
@Composable
private fun ReplyBar(
    writes: AgentWrites,
    blocked: Boolean,
    armed: Boolean,
    connected: Boolean,
) {
    val t = LocalTerm.current
    Column(
        Modifier
            .fillMaxWidth()
            .navigationBarsPadding()
            .imePadding(),
    ) {
        Rule()
        Column(
            Modifier.padding(horizontal = 16.dp, vertical = 10.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Text(
                if (blocked) "it's asking you — reply:" else "quick replies light up when it asks",
                color = if (blocked) t.red else t.dim,
                style = MaterialTheme.typography.bodySmall,
                fontWeight = if (blocked) FontWeight.Bold else null,
            )
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                listOf("1" to "1", "2" to "2", "3" to "3", "⏎" to "enter", "esc" to "esc").forEach { (label, key) ->
                    KeyCap(
                        label,
                        onClick = { writes.key(key) },
                        enabled = armed,
                        hot = blocked,
                        contentDescription = if (key == "enter") "Enter" else null,
                        modifier = Modifier.weight(1f),
                    )
                }
            }
        }
        Rule()
        PromptLine(writes, connected = connected, blocked = blocked)
    }
}
