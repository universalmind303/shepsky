package dev.collie.ui

import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import dev.collie.core.CollieRepo
import dev.collie.core.ConnState
import dev.collie.core.UserText
import dev.collie.data.Prefs

/** Shown whenever the phone ↔ Mac ↔ herdr chain isn't fully up. */
@Composable
fun ConnBanner(onOpenSettings: (() -> Unit)? = null) {
    val conn by CollieRepo.conn.collectAsState()
    val state by CollieRepo.state.collectAsState()
    val context = LocalContext.current
    val text = UserText.connection(conn, Prefs.url(context))
        ?: if (!state.herdr_connected) "Your Mac's bridge is reconnecting to herdr…" else return
    Banner(
        text,
        action = if (conn == ConnState.TokenRejected) onOpenSettings?.let { "Re-pair" to it } else null,
    )
}

@Composable
private fun Banner(text: String, action: Pair<String, () -> Unit>?) {
    Surface(color = MaterialTheme.colorScheme.errorContainer, modifier = Modifier.fillMaxWidth()) {
        Row(
            verticalAlignment = Alignment.CenterVertically,
            modifier = Modifier.padding(start = 20.dp, end = 8.dp, top = 4.dp, bottom = 4.dp),
        ) {
            Text(
                text,
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onErrorContainer,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f).padding(vertical = 4.dp),
            )
            action?.let { (label, onClick) ->
                TextButton(onClick = onClick) { Text(label) }
            }
        }
    }
}
