package dev.collie.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import dev.collie.core.AgentView
import dev.collie.core.CollieRepo
import dev.collie.core.ConnState

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun OverviewScreen(
    onOpenAgent: (String) -> Unit,
    onOpenSettings: () -> Unit,
) {
    val state by CollieRepo.state.collectAsState()
    val conn by CollieRepo.conn.collectAsState()

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("collie") },
                actions = {
                    IconButton(onClick = onOpenSettings) {
                        Icon(Icons.Default.Settings, contentDescription = "Settings")
                    }
                },
            )
        },
    ) { padding ->
        Column(Modifier.fillMaxSize().padding(padding)) {
            if (conn !is ConnState.Connected) {
                ConnBanner(conn)
            } else if (!state.herdr_connected) {
                Banner("bridge is reconnecting to herdr…")
            }

            val byWorkspace = state.agents.groupBy { it.workspace_label ?: it.workspace_id }
            if (state.agents.isEmpty()) {
                Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                    Text(
                        if (conn is ConnState.Connected) "No agents running right now."
                        else "Waiting for the bridge…",
                        style = MaterialTheme.typography.bodyLarge,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            } else {
                LazyColumn(Modifier.fillMaxSize()) {
                    byWorkspace.forEach { (workspace, agents) ->
                        item(key = "ws-$workspace") {
                            Text(
                                workspace,
                                style = MaterialTheme.typography.labelLarge,
                                color = MaterialTheme.colorScheme.primary,
                                modifier = Modifier.padding(start = 20.dp, top = 18.dp, bottom = 6.dp),
                            )
                        }
                        items(agents.size, key = { agents[it].pane_id }) { index ->
                            AgentRow(agents[index], onClick = { onOpenAgent(agents[index].pane_id) })
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun AgentRow(agent: AgentView, onClick: () -> Unit) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clickable(onClick = onClick)
            .padding(horizontal = 20.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Box(
            Modifier
                .size(12.dp)
                .background(statusColor(agent.status), CircleShape)
        )
        Column(Modifier.weight(1f)) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(agent.label, style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
                Text(
                    statusLabel(agent.status),
                    style = MaterialTheme.typography.labelMedium,
                    color = statusColor(agent.status),
                )
            }
            agent.title?.takeIf { it.isNotBlank() }?.let {
                Text(
                    it,
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
    }
}

@Composable
private fun ConnBanner(conn: ConnState) {
    val text = when (conn) {
        ConnState.Idle -> "not connected — open Settings"
        ConnState.Connecting -> "connecting to the bridge…"
        is ConnState.Disconnected -> "reconnecting…" + (conn.detail?.let { " ($it)" } ?: "")
        ConnState.Connected -> return
    }
    Banner(text)
}

@Composable
private fun Banner(text: String) {
    Surface(color = MaterialTheme.colorScheme.errorContainer, modifier = Modifier.fillMaxWidth()) {
        Text(
            text,
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onErrorContainer,
            maxLines = 2,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.padding(horizontal = 20.dp, vertical = 8.dp),
        )
    }
}
