package dev.shepsky.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
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
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import dev.shepsky.core.AgentView
import dev.shepsky.core.ShepskyRepo
import dev.shepsky.core.ConnState
import dev.shepsky.data.Prefs
import kotlinx.coroutines.delay

@Composable
fun OverviewScreen(
    onOpenAgent: (String) -> Unit,
    onNewAgent: () -> Unit,
    onOpenSettings: () -> Unit,
) {
    val state by ShepskyRepo.state.collectAsState()
    val conn by ShepskyRepo.conn.collectAsState()
    // An older bridge doesn't offer starting agents.
    val canStart = ShepskyRepo.startKinds.collectAsState().value.isNotEmpty()
    val t = LocalTerm.current
    val count = state.agents.size
    val context = LocalContext.current
    var sort by remember { mutableStateOf(Prefs.overviewSort(context)) }

    Scaffold(
        topBar = {
            TopLine(
                // Theme last: on a narrow screen it's the part that can go.
                segments(
                    Seg("shepsky", t.accent, bold = true),
                    Seg(if (count == 1) "1 agent" else "$count agents"),
                    linkSeg(),
                    Seg(t.id, t.dim),
                ),
                actions = {
                    if (canStart) TapText("[+]", onNewAgent, contentDescription = "New agent")
                    TapText("[≡]", onOpenSettings, contentDescription = "Settings")
                },
            )
        },
    ) { padding ->
        Column(Modifier.fillMaxSize().padding(padding)) {
            ConnBanner(onOpenSettings)

            if (state.agents.isEmpty()) {
                Box(Modifier.fillMaxSize().padding(24.dp), contentAlignment = Alignment.Center) {
                    Column(horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(16.dp)) {
                        BlinkingLine(
                            if (conn is ConnState.Connected) "no agents running" else "waiting for your mac",
                        )
                        if (canStart && conn is ConnState.Connected) KeyCap("+ start an agent", onClick = onNewAgent)
                    }
                }
                return@Column
            }

            SortLine(sort) {
                sort = it
                Prefs.setOverviewSort(context, it)
            }

            // Labels line up in one column, like a herdr sidebar.
            val labelWidth = state.agents.maxOf { it.label.length }.coerceIn(5, 12)
            val workspaceOf = { agent: AgentView ->
                state.workspaces.firstOrNull { it.workspace_id == agent.workspace_id }
                    ?.let { workspaceName(it, state.workspaces) }
                    ?: agent.workspace_label ?: agent.workspace_id
            }
            // Whatever the order, agents waiting on you come first.
            val (waiting, rest) = state.agents.partition { it.status == "blocked" }
            val sectionPad = Modifier.padding(start = 16.dp, top = 18.dp, bottom = 4.dp)
            LazyColumn(Modifier.fillMaxSize()) {
                if (waiting.isNotEmpty()) {
                    item(key = "needs-you") { SectionRule("needs you", sectionPad, color = t.red) }
                    items(waiting.size, key = { waiting[it].pane_id }) { i ->
                        AgentRow(waiting[i], labelWidth, workspaceOf(waiting[i])) { onOpenAgent(waiting[i].pane_id) }
                    }
                }
                when (sort) {
                    Prefs.SORT_NAME, Prefs.SORT_RECENT -> {
                        val sorted = if (sort == Prefs.SORT_NAME) {
                            rest.sortedWith(compareBy({ it.label.lowercase() }, { workspaceOf(it).lowercase() }))
                        } else {
                            rest.sortedByDescending { it.state_change_seq }
                        }
                        if (waiting.isNotEmpty() && sorted.isNotEmpty()) {
                            item(key = "sorted") { SectionRule(if (sort == Prefs.SORT_NAME) "by name" else "recent", sectionPad) }
                        }
                        items(sorted.size, key = { sorted[it].pane_id }) { i ->
                            AgentRow(sorted[i], labelWidth, workspaceOf(sorted[i])) { onOpenAgent(sorted[i].pane_id) }
                        }
                    }
                    // By id: two workspaces can share a label (same directory).
                    else -> rest.groupBy { it.workspace_id }.forEach { (workspaceId, agents) ->
                        item(key = "ws-$workspaceId") { SectionRule(workspaceOf(agents.first()), sectionPad) }
                        items(agents.size, key = { agents[it].pane_id }) { i ->
                            AgentRow(agents[i], labelWidth) { onOpenAgent(agents[i].pane_id) }
                        }
                    }
                }
            }
        }
    }
}

/** `sort  name · recent · [space]`: the current order in brackets. */
@Composable
private fun SortLine(sort: String, onPick: (String) -> Unit) {
    val t = LocalTerm.current
    Row(Modifier.fillMaxWidth().padding(start = 16.dp, end = 4.dp), verticalAlignment = Alignment.CenterVertically) {
        Text("sort", color = t.dim, style = MaterialTheme.typography.bodySmall)
        listOf(Prefs.SORT_NAME, Prefs.SORT_RECENT, Prefs.SORT_SPACE).forEach { mode ->
            val on = mode == sort
            TapText(
                if (on) "[$mode]" else mode,
                onClick = { onPick(mode) },
                contentDescription = "Sort by $mode",
                color = if (on) t.accent else t.dim,
            )
        }
    }
    Rule()
}

/** One agent; [workspace] is shown when the list isn't grouped by it. */
@Composable
private fun AgentRow(agent: AgentView, labelWidth: Int, workspace: String? = null, onClick: () -> Unit) {
    val t = LocalTerm.current
    val color = statusColor(agent.status)
    Column(
        Modifier
            .fillMaxWidth()
            .heightIn(min = 64.dp)
            .clickable(onClick = onClick)
            .padding(horizontal = 16.dp, vertical = 12.dp),
    ) {
        Text(
            buildAnnotatedString {
                withStyle(SpanStyle(color = color)) { append("[${statusGlyph(agent.status)}] ") }
                withStyle(SpanStyle(fontWeight = FontWeight.Bold)) {
                    append(agent.label.take(labelWidth).padEnd(labelWidth))
                }
                withStyle(SpanStyle(color = color)) { append("  ${statusLabel(agent.status)}") }
            },
            style = MaterialTheme.typography.bodyMedium,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
        listOfNotNull(workspace, agent.title?.takeIf { it.isNotBlank() }).joinToString(" · ").takeIf { it.isNotEmpty() }?.let {
            Text(
                "    $it",
                color = t.dim,
                style = MaterialTheme.typography.bodySmall,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
    }
}

/** `text █`, the cursor blinking. */
@Composable
fun BlinkingLine(text: String) {
    val t = LocalTerm.current
    var on by remember { mutableStateOf(true) }
    LaunchedEffect(Unit) {
        while (true) {
            delay(530)
            on = !on
        }
    }
    Text(
        buildAnnotatedString {
            withStyle(SpanStyle(color = t.dim)) { append("$text ") }
            withStyle(SpanStyle(color = if (on) t.fg else t.bg)) { append("█") }
        },
        style = MaterialTheme.typography.bodyMedium,
    )
}
