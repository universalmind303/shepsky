package dev.shepsky.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
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
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import dev.shepsky.core.AgentView
import dev.shepsky.core.ShepskyRepo
import dev.shepsky.core.ConnState
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

            // Labels line up in one column, like a herdr sidebar.
            val labelWidth = state.agents.maxOf { it.label.length }.coerceIn(5, 12)
            // By id: two workspaces can share a label (same directory).
            val byWorkspace = state.agents.groupBy { it.workspace_id }
            LazyColumn(Modifier.fillMaxSize()) {
                byWorkspace.forEach { (workspaceId, agents) ->
                    item(key = "ws-$workspaceId") {
                        val ws = state.workspaces.firstOrNull { it.workspace_id == workspaceId }
                        SectionRule(
                            ws?.let { workspaceName(it, state.workspaces) } ?: agents.first().workspace_label ?: workspaceId,
                            Modifier.padding(start = 16.dp, top = 18.dp, bottom = 4.dp),
                        )
                    }
                    items(agents.size, key = { agents[it].pane_id }) { index ->
                        AgentRow(agents[index], labelWidth, onClick = { onOpenAgent(agents[index].pane_id) })
                    }
                }
            }
        }
    }
}

@Composable
private fun AgentRow(agent: AgentView, labelWidth: Int, onClick: () -> Unit) {
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
        agent.title?.takeIf { it.isNotBlank() }?.let {
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
