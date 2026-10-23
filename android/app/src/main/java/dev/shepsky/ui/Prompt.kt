package dev.shepsky.ui

import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.SnackbarHostState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.unit.dp
import dev.shepsky.core.OpResult
import dev.shepsky.core.ShepskyRepo
import kotlinx.coroutines.launch

/**
 * What a screen sends to one agent: its prompt box and key presses. The
 * prompt clears optimistically; if the bridge refuses it, the text comes
 * back (unless something new was typed) along with a plain-language reason.
 */
@Stable
class AgentWrites(private val paneId: String) {
    var prompt by mutableStateOf("")

    /** The prompt we cleared, until the bridge confirms it: op id to text. */
    private var sent: Pair<String, String>? = null
    private val keyOps = mutableSetOf<String>()

    fun send() {
        val text = prompt.trim()
        sent = ShepskyRepo.prompt(paneId, text) to text
        prompt = ""
    }

    fun key(key: String) {
        keyOps += ShepskyRepo.sendKeys(paneId, listOf(key))
    }

    /** The message to show if [result] is one of ours that failed. */
    fun onResult(result: OpResult): String? {
        val sent = sent
        val isPrompt = sent != null && sent.first == result.opId
        if (isPrompt) this.sent = null
        if (result !is OpResult.Failed) {
            keyOps.remove(result.opId)
            return null
        }
        when {
            isPrompt -> if (prompt.isBlank()) prompt = sent!!.second
            keyOps.remove(result.opId) -> {}
            else -> return null // e.g. a focus op: not worth a message
        }
        return result.friendly
    }
}

/** [AgentWrites] for [paneId], showing refusals in [snackbar]. */
@Composable
fun rememberAgentWrites(paneId: String, snackbar: SnackbarHostState): AgentWrites {
    val writes = remember(paneId) { AgentWrites(paneId) }
    val scope = rememberCoroutineScope()
    LaunchedEffect(writes) {
        ShepskyRepo.opResults.collect { result ->
            val message = writes.onResult(result) ?: return@collect
            scope.launch {
                snackbar.currentSnackbarData?.dismiss()
                snackbar.showSnackbar(message)
            }
        }
    }
    return writes
}

/** `> prompt [⏎]`: the agent's prompt box, disabled while it can't take one. */
@Composable
fun PromptLine(
    writes: AgentWrites,
    connected: Boolean,
    blocked: Boolean,
    focus: FocusRequester? = null,
) {
    val t = LocalTerm.current
    val canPrompt = connected && !blocked
    val canSend = canPrompt && writes.prompt.isNotBlank()
    Row(Modifier.padding(start = 16.dp, end = 8.dp), verticalAlignment = Alignment.CenterVertically) {
        TermField(
            value = writes.prompt,
            onValueChange = { writes.prompt = it },
            enabled = canPrompt,
            placeholder = when {
                !connected -> "not connected to your mac"
                blocked -> "answer its question first"
                else -> "send a prompt"
            },
            maxLines = 4,
            modifier = Modifier.weight(1f).then(focus?.let { Modifier.focusRequester(it) } ?: Modifier),
        )
        TapText(
            "[⏎]",
            writes::send,
            contentDescription = "Send",
            color = if (canSend) t.accent else t.dim,
            enabled = canSend,
        )
    }
}
