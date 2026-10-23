package dev.collie.ui

import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
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
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import dev.collie.core.CollieRepo
import dev.collie.core.ConnState
import dev.collie.core.Frame
import dev.collie.core.OpResult
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

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
    val state by CollieRepo.state.collectAsState()
    val conn by CollieRepo.conn.collectAsState()
    val frame by CollieRepo.frameFlow(paneId).collectAsState()
    val agent = state.agents.firstOrNull { it.pane_id == paneId }
    val connected = conn == ConnState.Connected
    val blocked = agent?.status == "blocked"

    val snackbar = remember { SnackbarHostState() }
    val scope = rememberCoroutineScope()
    var prompt by remember { mutableStateOf("") }
    // The prompt we cleared optimistically, until the bridge confirms it.
    var sentPrompt by remember { mutableStateOf<Pair<String, String>?>(null) } // op id to text
    val keyOps = remember { mutableSetOf<String>() }

    var armed by remember { mutableStateOf(false) }
    LaunchedEffect(blocked) {
        armed = false
        if (blocked) {
            delay(ARM_DELAY_MS)
            armed = true
        }
    }

    DisposableEffect(paneId) {
        CollieRepo.watchPane(paneId)
        onDispose { CollieRepo.unwatchPane(paneId) }
    }
    // Looking at a finished agent clears "finished". Only then: agent.focus
    // also switches herdr on the Mac to this agent's tab.
    LaunchedEffect(paneId, connected, agent?.status) {
        if (connected && agent?.status == "done") CollieRepo.focusAgent(paneId)
    }
    LaunchedEffect(Unit) {
        CollieRepo.opResults.collect { result ->
            val sent = sentPrompt
            val isPrompt = sent != null && sent.first == result.opId
            if (isPrompt) sentPrompt = null
            if (result !is OpResult.Failed) {
                keyOps.remove(result.opId)
                return@collect
            }
            when {
                // Give the text back unless they've started typing something new.
                isPrompt -> if (prompt.isBlank()) prompt = sent!!.second
                keyOps.remove(result.opId) -> {}
                else -> return@collect // e.g. our focus op: not worth a snackbar
            }
            scope.launch {
                snackbar.currentSnackbarData?.dismiss()
                snackbar.showSnackbar(result.friendly)
            }
        }
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
                ReplyBar(
                    blocked = blocked,
                    armed = armed && connected,
                    connected = connected,
                    prompt = prompt,
                    onPromptChange = { prompt = it },
                    onKey = { key -> keyOps += CollieRepo.sendKeys(paneId, listOf(key)) },
                    onSend = {
                        val text = prompt.trim()
                        sentPrompt = CollieRepo.prompt(paneId, text) to text
                        prompt = ""
                    },
                )
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
                TerminalLines(lines, Modifier.horizontalScroll(rememberScrollState()))
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
    blocked: Boolean,
    armed: Boolean,
    connected: Boolean,
    prompt: String,
    onPromptChange: (String) -> Unit,
    onKey: (String) -> Unit,
    onSend: () -> Unit,
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
                        onClick = { onKey(key) },
                        enabled = armed,
                        hot = blocked,
                        contentDescription = if (key == "enter") "Enter" else null,
                        modifier = Modifier.weight(1f),
                    )
                }
            }
        }
        Rule()
        val canPrompt = connected && !blocked
        Row(
            Modifier.padding(start = 16.dp, end = 8.dp, top = 4.dp, bottom = 4.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            TermField(
                value = prompt,
                onValueChange = onPromptChange,
                enabled = canPrompt,
                placeholder = when {
                    !connected -> "not connected to your mac"
                    blocked -> "answer its question first"
                    else -> "send a prompt"
                },
                maxLines = 4,
                modifier = Modifier.weight(1f).padding(vertical = 12.dp),
            )
            TapText(
                "[⏎]",
                onSend,
                contentDescription = "Send",
                color = if (canPrompt && prompt.isNotBlank()) t.accent else t.dim,
                enabled = canPrompt && prompt.isNotBlank(),
            )
        }
    }
}
