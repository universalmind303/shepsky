package dev.collie.ui

import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
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
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import dev.collie.core.CollieRepo
import dev.collie.core.ConnState
import dev.collie.core.Frame
import dev.collie.core.OpResult
import kotlinx.coroutines.launch

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
    val t = LocalTerm.current
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
            TopLine(
                segments(
                    Seg(agent?.label ?: "terminal", bold = true),
                    *listOfNotNull(
                        agent?.workspace_label?.let { Seg(it, t.dim) },
                        agent?.let { Seg("[${statusGlyph(it.status)}] ${statusLabel(it.status)}", statusColor(it.status)) },
                    ).toTypedArray(),
                ),
                onBack = onBack,
            )
        },
        bottomBar = {
            KeyBar(enabled = conn == ConnState.Connected) { key ->
                keyOps += CollieRepo.sendKeys(paneId, listOf(key))
            }
        },
        snackbarHost = { TermSnackbarHost(snackbar) },
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
            .verticalScroll(vScroll)
            .horizontalScroll(hScroll)
            .padding(10.dp),
    ) {
        val current = frame
        if (current == null) {
            BlinkingLine("waiting for frames")
        } else {
            TerminalLines(current.lines)
        }
    }
}

@Composable
private fun KeyBar(enabled: Boolean, onKey: (String) -> Unit) {
    Column(Modifier.fillMaxWidth().navigationBarsPadding()) {
        Rule()
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .horizontalScroll(rememberScrollState())
                .padding(horizontal = 12.dp, vertical = 8.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            listOf(
                "esc" to "esc",
                "tab" to "tab",
                "↑" to "up",
                "↓" to "down",
                "←" to "left",
                "→" to "right",
                "⏎" to "enter",
            ).forEach { (label, key) ->
                KeyCap(label, onClick = { onKey(key) }, enabled = enabled)
            }
        }
    }
}
