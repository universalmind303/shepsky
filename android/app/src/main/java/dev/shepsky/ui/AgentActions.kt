package dev.shepsky.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import dev.shepsky.core.AgentView
import dev.shepsky.core.OpResult
import dev.shepsky.core.ShepskyRepo
import dev.shepsky.core.UserText

/** herdr's rule for agent names. */
private val AGENT_NAME = Regex("[a-z][a-z0-9_-]{0,31}")

private enum class Step { Menu, Rename, Close }

/**
 * Rename or close an agent: a small flat popup. Renaming sets herdr's agent
 * name and its pane's label; closing ends the agent and asks first.
 * [onClosed] runs once the bridge confirms the close.
 */
@Composable
fun AgentActions(agent: AgentView, workspace: String?, onDismiss: () -> Unit, onClosed: () -> Unit = onDismiss) {
    val t = LocalTerm.current
    var step by remember { mutableStateOf(Step.Menu) }
    var name by remember { mutableStateOf(agent.label) }
    var opId by remember { mutableStateOf<String?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    val who = listOfNotNull(agent.label, workspace).joinToString(" · ")

    LaunchedEffect(Unit) {
        ShepskyRepo.opResults.collect { result ->
            if (result.opId != opId) return@collect
            opId = null
            when (result) {
                is OpResult.Ok -> if (step == Step.Close) onClosed() else onDismiss()
                is OpResult.Failed -> error = UserText.manageError(result.code, result.message)
            }
        }
    }

    Dialog(onDismissRequest = onDismiss) {
        Column(
            Modifier
                .fillMaxWidth()
                .background(t.bg)
                .border(1.dp, if (step == Step.Close) t.red else t.dim)
                .padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            // Explicit colors: a dialog doesn't inherit the screen's content color.
            Text(who, color = t.fg, fontWeight = FontWeight.Bold, style = MaterialTheme.typography.bodyMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
            when (step) {
                Step.Menu -> Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    KeyCap("rename", onClick = { step = Step.Rename }, modifier = Modifier.weight(1f))
                    KeyCap("close", onClick = { step = Step.Close }, modifier = Modifier.weight(1f))
                }
                Step.Rename -> {
                    val valid = AGENT_NAME.matches(name)
                    TermField(
                        value = name,
                        // herdr's names are lowercase, no spaces.
                        onValueChange = {
                            name = it.lowercase().replace(' ', '-')
                            error = null
                        },
                        enabled = opId == null,
                        keyboardOptions = KeyboardOptions(
                            capitalization = KeyboardCapitalization.None,
                            autoCorrectEnabled = false,
                            keyboardType = KeyboardType.Ascii,
                        ),
                        modifier = Modifier.fillMaxWidth(),
                    )
                    Rule()
                    Text(
                        "lowercase letters, digits, - or _; starts with a letter; up to 32",
                        color = if (valid || name.isEmpty()) t.dim else t.red,
                        style = MaterialTheme.typography.bodySmall,
                    )
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        KeyCap("cancel", onClick = onDismiss, modifier = Modifier.weight(1f))
                        KeyCap(
                            if (opId != null) "renaming…" else "rename",
                            onClick = {
                                error = null
                                opId = ShepskyRepo.renameAgent(agent.pane_id, name)
                            },
                            enabled = valid && name != agent.label && opId == null,
                            modifier = Modifier.weight(1f),
                        )
                    }
                }
                Step.Close -> {
                    Text(
                        "close $who? this ends the agent.",
                        color = t.bg,
                        fontWeight = FontWeight.Bold,
                        style = MaterialTheme.typography.bodyMedium,
                        modifier = Modifier.fillMaxWidth().background(t.red).padding(10.dp),
                    )
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        KeyCap("cancel", onClick = onDismiss, modifier = Modifier.weight(1f))
                        KeyCap(
                            if (opId != null) "closing…" else "close",
                            onClick = {
                                error = null
                                opId = ShepskyRepo.closeAgent(agent.pane_id)
                            },
                            enabled = opId == null,
                            hot = true,
                            modifier = Modifier.weight(1f),
                        )
                    }
                }
            }
            error?.let { Text(it, color = t.red, style = MaterialTheme.typography.bodySmall) }
        }
    }
}
