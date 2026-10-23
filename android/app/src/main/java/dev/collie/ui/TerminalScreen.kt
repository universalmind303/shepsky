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

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun TerminalScreen(paneId: String, onBack: () -> Unit) {
    DisposableEffect(paneId) {
        CollieRepo.watchPane(paneId)
        onDispose { CollieRepo.unwatchPane(paneId) }
    }
    val frame by CollieRepo.frameFlow(paneId).collectAsState()
    val vScroll = rememberScrollState()
    val hScroll = rememberScrollState()

    // Stick to the bottom as new frames land.
    LaunchedEffect(frame) { vScroll.scrollTo(vScroll.maxValue) }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(paneId, fontFamily = FontFamily.Monospace) },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
                    }
                },
            )
        },
        bottomBar = { KeyBar(paneId) },
    ) { padding ->
        Box(
            modifier = Modifier
                .fillMaxSize()
                .padding(padding)
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
}

@Composable
private fun KeyBar(paneId: String) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .background(MaterialTheme.colorScheme.surface)
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
                onClick = { CollieRepo.sendKeys(paneId, listOf(key)) },
                modifier = Modifier.padding(horizontal = 4.dp),
            ) {
                Text(label)
            }
        }
    }
}
