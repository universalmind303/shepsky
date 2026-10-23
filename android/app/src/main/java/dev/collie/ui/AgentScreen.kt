package dev.collie.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.Send
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import dev.collie.core.CollieRepo

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun AgentScreen(
    paneId: String,
    onOpenTerminal: () -> Unit,
    onBack: () -> Unit,
) {
    val state by CollieRepo.state.collectAsState()
    val agent = state.agents.firstOrNull { it.pane_id == paneId }
    val snackbar = remember { SnackbarHostState() }
    var prompt by remember { mutableStateOf("") }

    // Opening the agent counts as looking at it (clears "finished").
    LaunchedEffect(paneId) { CollieRepo.focusAgent(paneId) }
    LaunchedEffect(Unit) {
        CollieRepo.opErrors.collect { snackbar.showSnackbar(it) }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(agent?.label ?: "agent") },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
                    }
                },
            )
        },
        snackbarHost = { SnackbarHost(snackbar) },
    ) { padding ->
        if (agent == null) {
            Box(Modifier.fillMaxSize().padding(padding), contentAlignment = Alignment.Center) {
                Text("This agent is gone.", color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
            return@Scaffold
        }

        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(padding)
                .padding(horizontal = 20.dp)
                .verticalScroll(rememberScrollState()),
            verticalArrangement = Arrangement.spacedBy(14.dp),
        ) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Box(Modifier.size(12.dp).background(statusColor(agent.status), CircleShape))
                Text(statusLabel(agent.status), color = statusColor(agent.status), style = MaterialTheme.typography.titleMedium)
                agent.workspace_label?.let {
                    Text("· $it", color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
            agent.title?.takeIf { it.isNotBlank() }?.let {
                Text(it, style = MaterialTheme.typography.bodyLarge)
            }
            agent.cwd?.let {
                Text(
                    it,
                    style = MaterialTheme.typography.bodySmall.copy(fontFamily = FontFamily.Monospace),
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }

            if (agent.status == "blocked") {
                Text("Quick reply", style = MaterialTheme.typography.labelLarge)
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    listOf("1", "2", "3", "enter", "esc").forEach { key ->
                        OutlinedButton(onClick = { CollieRepo.sendKeys(paneId, listOf(key)) }) {
                            Text(if (key.length == 1) key else key.replaceFirstChar { it.uppercase() })
                        }
                    }
                }
            }

            OutlinedTextField(
                value = prompt,
                onValueChange = { prompt = it },
                label = { Text("Prompt ${agent.label}") },
                trailingIcon = {
                    IconButton(
                        onClick = {
                            CollieRepo.prompt(paneId, prompt.trim())
                            prompt = ""
                        },
                        enabled = prompt.isNotBlank(),
                    ) {
                        Icon(Icons.AutoMirrored.Filled.Send, contentDescription = "Send")
                    }
                },
                minLines = 2,
                modifier = Modifier.fillMaxWidth(),
            )

            Button(onClick = onOpenTerminal, modifier = Modifier.fillMaxWidth()) {
                Text("Open terminal")
            }
        }
    }
}
