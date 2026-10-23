package dev.collie.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.collie.core.CollieRepo
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.text.style.TextOverflow
import dev.collie.core.ConnState
import dev.collie.core.Frame
import dev.collie.core.OpResult
import kotlinx.coroutines.launch

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun TerminalScreen(paneId: String, onOpenSettings: () -> Unit, onBack: () -> Unit) {
    DisposableEffect(paneId) {
        CollieRepo.watchPane(paneId)
        onDispose { CollieRepo.unwatchPane(paneId) }
    }
    val frame by CollieRepo.frameFlow(paneId).collectAsState()
    val state by CollieRepo.state.collectAsState()
    val conn by CollieRepo.conn.collectAsState()
    val agent = state.agents.firstOrNull { it.pane_id == paneId }
    val snackbar = remember { SnackbarHostState() }
    val scope = rememberCoroutineScope()
    val keyOps = remember { mutableSetOf<String>() }
    LaunchedEffect(Unit) {
        CollieRepo.opResults.collect { result ->
            if (!keyOps.remove(result.opId) || result !is OpResult.Failed) return@collect
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
                    Text(
                        listOfNotNull(agent?.label ?: "terminal", agent?.workspace_label).joinToString(" · "),
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
                    }
                },
            )
        },
        bottomBar = {
            KeyBar(enabled = conn == ConnState.Connected) { key ->
                keyOps += CollieRepo.sendKeys(paneId, listOf(key))
            }
        },
        snackbarHost = { SnackbarHost(snackbar) },
    ) { padding ->
        Column(Modifier.fillMaxSize().padding(padding)) {
            ConnBanner(onOpenSettings)
            TerminalView(frame)
        }
    }
}

@Composable
private fun TerminalView(frame: Frame?) {
    val vScroll = rememberScrollState()
    val hScroll = rememberScrollState()

    // Stick to the bottom as new frames land.
    LaunchedEffect(frame) { vScroll.scrollTo(vScroll.maxValue) }

    Box(
        modifier = Modifier
            .fillMaxSize()
            .background(TerminalBackground)
            .verticalScroll(vScroll)
            .horizontalScroll(hScroll)
            .padding(10.dp),
    ) {
        val current = frame
        if (current == null) {
            Text(
                "waiting for frames…",
                color = TerminalDim,
                fontFamily = FontFamily.Monospace,
                fontSize = 12.sp,
            )
        } else {
            TerminalLines(current.lines)
        }
    }
}

@Composable
private fun KeyBar(enabled: Boolean, onKey: (String) -> Unit) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .background(MaterialTheme.colorScheme.surface)
            .navigationBarsPadding()
            .horizontalScroll(rememberScrollState())
            .padding(horizontal = 8.dp, vertical = 6.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        listOf(
            "Esc" to "esc",
            "Tab" to "tab",
            "↑" to "up",
            "↓" to "down",
            "←" to "left",
            "→" to "right",
            "⏎" to "enter",
        ).forEach { (label, key) ->
            OutlinedButton(
                onClick = { onKey(key) },
                enabled = enabled,
                modifier = Modifier.padding(horizontal = 4.dp),
            ) {
                Text(label)
            }
        }
    }
}
