package dev.shepsky.ui

import androidx.compose.foundation.clickable
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
    onOpenSettings: () -> Unit,
) {
    val state by ShepskyRepo.state.collectAsState()
    val conn by ShepskyRepo.conn.collectAsState()
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
                actions = { TapText("[≡]", onOpenSettings, contentDescription = "Settings") },
            )
        },
    ) { padding ->
        Column(Modifier.fillMaxSize().padding(padding)) {
            ConnBanner(onOpenSettings)

            if (state.agents.isEmpty()) {
                Box(Modifier.fillMaxSize().padding(24.dp), contentAlignment = Alignment.Center) {
                    BlinkingLine(
                        if (conn is ConnState.Connected) "no agents running" else "waiting for your mac",
                    )
                }
                return@Column
            }

            // Labels line up in one column, like a herdr sidebar.
            val labelWidth = state.agents.maxOf { it.label.length }.coerceIn(5, 12)
            val byWorkspace = state.agents.groupBy { it.workspace_label ?: it.workspace_id }
            LazyColumn(Modifier.fillMaxSize()) {
                byWorkspace.forEach { (workspace, agents) ->
                    item(key = "ws-$workspace") {
                        SectionRule(workspace, Modifier.padding(start = 16.dp, top = 14.dp, bottom = 2.dp))
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
            .heightIn(min = 56.dp)
            .clickable(onClick = onClick)
            .padding(horizontal = 16.dp, vertical = 8.dp),
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
