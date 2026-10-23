package dev.shepsky.ui

import androidx.activity.compose.BackHandler
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.platform.LocalContext
import dev.shepsky.MainActivity
import dev.shepsky.PairLink
import dev.shepsky.core.ShepskyRepo
import dev.shepsky.data.Prefs
import dev.shepsky.service.ShepskyService

sealed interface Screen {
    data object Overview : Screen
    data class Agent(val paneId: String) : Screen
}

@Composable
fun App() {
    val context = LocalContext.current
    var showPair by remember { mutableStateOf(!Prefs.isPaired(context)) }
    var pairPrefill by remember { mutableStateOf<PairLink?>(null) }
    var stack by remember { mutableStateOf<List<Screen>>(listOf(Screen.Overview)) }

    // Reconnect the service on app open (e.g. after a reboot).
    LaunchedEffect(Unit) {
        if (Prefs.isPaired(context) && !ShepskyRepo.running) {
            ShepskyService.start(context)
        }
    }

    // Notification taps jump straight to the agent.
    val pendingPane by MainActivity.pendingPane.collectAsState()
    LaunchedEffect(pendingPane) {
        pendingPane?.let { paneId ->
            MainActivity.pendingPane.value = null
            if (Prefs.isPaired(context)) {
                stack = listOf(Screen.Overview, Screen.Agent(paneId))
                showPair = false
            }
        }
    }

    // A pairing link opens the pair screen pre-filled (never auto-connects).
    val pendingPair by MainActivity.pendingPair.collectAsState()
    LaunchedEffect(pendingPair) {
        pendingPair?.let { link ->
            MainActivity.pendingPair.value = null
            // Re-running the demo re-sends the same link; don't yank a
            // working app over to Settings for it.
            val alreadyLive = ShepskyRepo.running &&
                link.url == Prefs.url(context) && link.token == Prefs.token(context)
            if (!alreadyLive) {
                pairPrefill = link
                showPair = true
            }
        }
    }

    if (showPair) {
        val close = {
            showPair = false
            pairPrefill = null
        }
        BackHandler(enabled = Prefs.isPaired(context)) { close() }
        PairScreen(
            prefill = pairPrefill,
            canDismiss = Prefs.isPaired(context),
            onDismiss = close,
            onPaired = close,
        )
        return
    }

    BackHandler(enabled = stack.size > 1) { stack = stack.dropLast(1) }

    when (val screen = stack.last()) {
        is Screen.Overview -> OverviewScreen(
            onOpenAgent = { stack = stack + Screen.Agent(it) },
            onOpenSettings = { showPair = true },
        )
        is Screen.Agent -> AgentScreen(
            paneId = screen.paneId,
            onOpenSettings = { showPair = true },
            onBack = { stack = stack.dropLast(1) },
        )
    }
}
