package dev.shepsky.ui

import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import dev.shepsky.core.ConnState
import dev.shepsky.core.OpResult
import dev.shepsky.core.ShepskyRepo
import dev.shepsky.core.WorkspaceView
import dev.shepsky.data.Prefs
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.withTimeoutOrNull

/** How long a started agent gets to show up in herdr's state. */
private const val APPEAR_TIMEOUT_MS = 20_000L

private const val MAX_SUGGESTIONS = 6

private val plainKeyboard = KeyboardOptions(
    capitalization = KeyboardCapitalization.None,
    autoCorrectEnabled = false,
    keyboardType = KeyboardType.Uri,
)

/**
 * Start an agent: which kind, where (a new workspace in a directory, or a
 * new tab in a workspace you have), and optionally its name. The bridge on
 * the Mac decides what's allowed; this screen just asks. Once the agent
 * shows up, [onStarted] opens it.
 */
@Composable
fun NewAgentScreen(onStarted: (paneId: String) -> Unit, onOpenSettings: () -> Unit, onBack: () -> Unit) {
    val context = LocalContext.current
    val state by ShepskyRepo.state.collectAsState()
    val conn by ShepskyRepo.conn.collectAsState()
    val kinds by ShepskyRepo.startKinds.collectAsState()
    val connected = conn == ConnState.Connected
    val t = LocalTerm.current

    var chosenKind by remember { mutableStateOf(Prefs.lastStartKind(context)) }
    val kind = chosenKind?.takeIf { it in kinds } ?: kinds.firstOrNull()
    /** null = a new workspace. */
    var workspaceId by remember { mutableStateOf<String?>(null) }
    var directory by remember { mutableStateOf("") }
    var name by remember { mutableStateOf("") }

    var opId by remember { mutableStateOf<String?>(null) }
    var startedPane by remember { mutableStateOf<String?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    val busy = opId != null || startedPane != null

    LaunchedEffect(Unit) {
        ShepskyRepo.opResults.collect { result ->
            if (result.opId != opId) return@collect
            opId = null
            when (result) {
                is OpResult.Ok -> startedPane = result.paneId
                is OpResult.Failed -> error = result.friendlyStart
            }
        }
    }
    // The ack means herdr started it; open it once it's in the state.
    LaunchedEffect(startedPane) {
        val pane = startedPane ?: return@LaunchedEffect
        val appeared = withTimeoutOrNull(APPEAR_TIMEOUT_MS) {
            ShepskyRepo.state.first { s -> s.agents.any { it.pane_id == pane } }
        }
        if (appeared != null) {
            onStarted(pane)
        } else {
            error = "It started, but herdr hasn't reported it yet. Look for it in the overview."
        }
    }
    // An error is about what was asked; changing the request clears it.
    LaunchedEffect(kind, workspaceId, directory, name) {
        if (opId == null && startedPane == null) error = null
    }
    // A workspace that closes while you're choosing falls back to "new".
    if (workspaceId != null && state.workspaces.none { it.workspace_id == workspaceId }) workspaceId = null

    val where = workspaceId?.let { id -> state.workspaces.firstOrNull { it.workspace_id == id } }
    val canStart = connected && kind != null && !busy && (workspaceId != null || directory.isNotBlank())
    val start = {
        val k = kind!!
        Prefs.setLastStartKind(context, k)
        error = null
        opId = ShepskyRepo.startAgent(
            kind = k,
            workspaceId = workspaceId,
            cwd = directory.trim().takeIf { workspaceId == null },
            name = name.trim().ifEmpty { null },
        )
    }

    Scaffold(
        topBar = {
            TopLine(
                segments(Seg("new agent", bold = true), linkSeg()),
                onBack = onBack,
            )
        },
        bottomBar = {
            Column(Modifier.fillMaxWidth().navigationBarsPadding().imePadding()) {
                Rule()
                error?.let { AlertLine(it) }
                KeyCap(
                    when {
                        startedPane != null -> "started · opening…"
                        opId != null -> "starting ${kind.orEmpty()}…"
                        else -> "start ${kind.orEmpty()}" + (where?.let { " in ${workspaceName(it, state.workspaces)}" } ?: "")
                    },
                    onClick = start,
                    enabled = canStart,
                    modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 10.dp),
                )
            }
        },
    ) { padding ->
        Column(Modifier.fillMaxSize().padding(padding)) {
            ConnBanner(onOpenSettings)
            if (connected && kinds.isEmpty()) {
                Text(
                    "The bridge on your Mac can't start agents. Update shepsky-bridge to get this.",
                    color = t.yellow,
                    style = MaterialTheme.typography.bodyMedium,
                    modifier = Modifier.padding(16.dp),
                )
                return@Column
            }
            Column(
                Modifier
                    .fillMaxSize()
                    .verticalScroll(rememberScrollState())
                    .padding(horizontal = 16.dp, vertical = 12.dp),
                verticalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                SectionRule("agent")
                Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    kinds.forEach { k ->
                        KeyCap(
                            k,
                            onClick = { chosenKind = k },
                            enabled = !busy,
                            selected = k == kind,
                            modifier = Modifier.weight(1f),
                        )
                    }
                }

                SectionRule("where", Modifier.padding(top = 14.dp))
                Choice("a new workspace", selected = workspaceId == null, enabled = !busy) { workspaceId = null }
                // Its directory right under it, above however many workspaces you have.
                if (workspaceId == null) {
                    Column(Modifier.padding(start = 12.dp, bottom = 6.dp)) {
                        TermField(
                            value = directory,
                            onValueChange = { directory = it },
                            enabled = !busy,
                            placeholder = "directory on your mac",
                            keyboardOptions = plainKeyboard,
                            modifier = Modifier.fillMaxWidth(),
                        )
                        Rule()
                        Places(state.places, typed = directory, enabled = !busy) { directory = it }
                    }
                }
                state.workspaces.forEach { ws ->
                    Choice(
                        "new tab in ${workspaceName(ws, state.workspaces)}",
                        selected = workspaceId == ws.workspace_id,
                        enabled = !busy,
                    ) { workspaceId = ws.workspace_id }
                }

                SectionRule("name", Modifier.padding(top = 14.dp))
                TermField(
                    // herdr's names are lowercase, no spaces.
                    value = name,
                    onValueChange = { name = it.lowercase().replace(' ', '-') },
                    enabled = !busy,
                    placeholder = "optional: one is picked for you",
                    keyboardOptions = plainKeyboard.copy(keyboardType = KeyboardType.Ascii),
                    modifier = Modifier.fillMaxWidth(),
                )
                Rule()
            }
        }
    }
}

/** A workspace's label, with herdr's number when another has the same label. */
internal fun workspaceName(ws: WorkspaceView, all: List<WorkspaceView>): String {
    val label = ws.label ?: return ws.workspace_id
    val shared = all.count { it.label == label } > 1
    return if (shared) "$label · ${ws.number ?: ws.workspace_id}" else label
}

/** `(•) label`: one of a set. */
@Composable
private fun Choice(label: String, selected: Boolean, enabled: Boolean, onClick: () -> Unit) {
    val t = LocalTerm.current
    Row(
        Modifier
            .fillMaxWidth()
            .heightIn(min = 48.dp)
            .border(1.dp, if (selected) t.accent else t.dim.copy(alpha = 0.4f))
            .clickable(enabled = enabled, onClick = onClick)
            .padding(horizontal = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            buildAnnotatedString {
                withStyle(SpanStyle(color = t.accent)) { append(if (selected) "(•) " else "( ) ") }
                withStyle(SpanStyle(color = if (enabled) t.fg else t.dim)) { append(label) }
            },
            style = MaterialTheme.typography.bodyMedium,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
    }
}

/** Directories your agents already work in, narrowed by what's typed. */
@Composable
private fun Places(places: List<String>, typed: String, enabled: Boolean, onPick: (String) -> Unit) {
    val t = LocalTerm.current
    val query = typed.trim()
    val shown = places.filter { it != query && it.contains(query, ignoreCase = true) }.take(MAX_SUGGESTIONS)
    Column {
        shown.forEach { place ->
            Text(
                "  $place",
                color = if (enabled) t.fg else t.dim,
                style = MaterialTheme.typography.bodyMedium,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier
                    .fillMaxWidth()
                    .heightIn(min = 44.dp)
                    .clickable(enabled = enabled) { onPick(place) }
                    .padding(vertical = 12.dp),
            )
        }
    }
}
