package dev.collie.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.Send
import androidx.compose.material.icons.filled.Terminal
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
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
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
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

@OptIn(ExperimentalMaterial3Api::class)
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

    Scaffold(
        topBar = {
            TopAppBar(
                title = {
                    Column {
                        Text(agent?.label ?: "agent", maxLines = 1, overflow = TextOverflow.Ellipsis)
                        agent?.workspace_label?.let {
                            Text(
                                it,
                                style = MaterialTheme.typography.labelMedium,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                    }
                },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
                    }
                },
                actions = {
                    if (agent != null) {
                        IconButton(onClick = onOpenTerminal) {
                            Icon(Icons.Default.Terminal, contentDescription = "Open terminal")
                        }
                    }
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
        snackbarHost = { SnackbarHost(snackbar) },
    ) { padding ->
        Column(Modifier.fillMaxSize().padding(padding)) {
            ConnBanner(onOpenSettings)
            if (agent == null) {
                Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                    Text(
                        if (connected && state.herdr_connected) "This agent is gone." else "Waiting for your Mac…",
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                return@Column
            }
            Column(
                modifier = Modifier
                    .fillMaxSize()
                    .verticalScroll(rememberScrollState())
                    .padding(horizontal = 16.dp, vertical = 12.dp),
                verticalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Box(Modifier.size(12.dp).background(statusColor(agent.status), CircleShape))
                    Text(statusLabel(agent.status), color = statusColor(agent.status), style = MaterialTheme.typography.titleMedium)
                }
                agent.title?.takeIf { it.isNotBlank() }?.let {
                    Text(
                        it,
                        style = MaterialTheme.typography.bodyLarge,
                        fontWeight = if (blocked) FontWeight.SemiBold else null,
                    )
                }
                OutputPreview(frame, onClick = onOpenTerminal)
                agent.cwd?.let {
                    Text(
                        it,
                        style = MaterialTheme.typography.bodySmall.copy(fontFamily = FontFamily.Monospace),
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
        }
    }
}

/** The last screenful of output, so nobody answers a question blind. */
@Composable
private fun OutputPreview(frame: Frame?, onClick: () -> Unit) {
    Column(
        Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(12.dp))
            .background(TerminalBackground)
            .clickable(onClick = onClick)
            .padding(12.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        val lines = frame?.lines?.dropLastWhile { it.isBlankLine() }?.takeLast(PREVIEW_LINES)
        if (lines == null) {
            Text("waiting for output…", color = TerminalDim, fontFamily = FontFamily.Monospace, fontSize = 12.sp)
        } else {
            TerminalLines(lines, Modifier.horizontalScroll(rememberScrollState()))
        }
        Text("Tap for the full terminal", color = TerminalDim, style = MaterialTheme.typography.labelSmall)
    }
}

/**
 * Quick replies over the prompt box. Fixed layout whatever the status: the
 * buttons only light up while the agent is asking, so nothing shifts under a
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
    val asking = statusColor("blocked")
    Surface(tonalElevation = 3.dp) {
        Column(
            Modifier
                .fillMaxWidth()
                .navigationBarsPadding()
                .imePadding()
                .padding(horizontal = 16.dp, vertical = 10.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Text(
                if (blocked) "It's asking you — reply:" else "Quick replies light up when it asks you something",
                style = MaterialTheme.typography.labelMedium,
                color = if (blocked) asking else MaterialTheme.colorScheme.onSurfaceVariant,
                fontWeight = if (blocked) FontWeight.SemiBold else null,
            )
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                listOf("1" to "1", "2" to "2", "3" to "3", "Enter" to "enter", "Esc" to "esc").forEach { (label, key) ->
                    Button(
                        onClick = { onKey(key) },
                        enabled = armed,
                        colors = ButtonDefaults.buttonColors(containerColor = asking, contentColor = Color.White),
                        contentPadding = PaddingValues(horizontal = 4.dp),
                        modifier = Modifier.weight(1f).height(48.dp),
                    ) {
                        Text(label, fontWeight = FontWeight.SemiBold)
                    }
                }
            }
            HorizontalDivider(Modifier.padding(vertical = 2.dp))
            val canPrompt = connected && !blocked
            OutlinedTextField(
                value = prompt,
                onValueChange = onPromptChange,
                enabled = canPrompt,
                placeholder = {
                    Text(
                        when {
                            !connected -> "Not connected to your Mac"
                            blocked -> "Answer its question first"
                            else -> "Send a prompt"
                        }
                    )
                },
                trailingIcon = {
                    IconButton(onClick = onSend, enabled = canPrompt && prompt.isNotBlank()) {
                        Icon(Icons.AutoMirrored.Filled.Send, contentDescription = "Send")
                    }
                },
                maxLines = 4,
                modifier = Modifier.fillMaxWidth(),
            )
        }
    }
}
