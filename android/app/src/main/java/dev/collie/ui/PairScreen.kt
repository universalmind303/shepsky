package dev.collie.ui

import android.Manifest
import android.os.Build
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import dev.collie.PairLink
import dev.collie.core.CollieRepo
import dev.collie.data.Prefs
import dev.collie.service.CollieService

@Composable
fun PairScreen(
    prefill: PairLink?,
    canDismiss: Boolean,
    onDismiss: () -> Unit,
    onPaired: () -> Unit,
) {
    val context = LocalContext.current
    val t = LocalTerm.current
    var url by remember(prefill) { mutableStateOf(prefill?.url ?: Prefs.url(context).ifBlank { "ws://" }) }
    var token by remember(prefill) { mutableStateOf(prefill?.token ?: Prefs.token(context)) }

    fun connect() {
        Prefs.save(context, url, token)
        CollieService.stop(context)
        CollieService.start(context)
        onPaired()
    }

    val askNotifications = rememberLauncherForActivityResult(
        ActivityResultContracts.RequestPermission()
    ) { connect() } // connect whether or not they allow notifications

    Scaffold(
        topBar = {
            TopLine(
                segments(
                    Seg("collie", t.accent, bold = true),
                    Seg(if (canDismiss) "settings" else "pair with your mac"),
                ),
                onBack = if (canDismiss) onDismiss else null,
            )
        },
    ) { padding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(padding)
                .verticalScroll(rememberScrollState())
                .navigationBarsPadding()
                .padding(horizontal = 16.dp, vertical = 12.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            SectionRule("pair")
            Text(
                buildAnnotatedString {
                    append("On your Mac, run ")
                    withStyle(SpanStyle(color = t.accent)) { append("collie-bridge pair") }
                    append(" and enter the URL and token it prints.")
                },
                style = MaterialTheme.typography.bodyMedium,
            )
            if (prefill != null) {
                Text(
                    "Filled in from a pairing link. Check that the URL is your Mac, then connect.",
                    color = t.yellow,
                    style = MaterialTheme.typography.bodySmall,
                )
            }

            FieldLabel("url", hint = "emulator: ws://10.0.2.2:7717/ws · tailscale: wss://<mac>.<tailnet>.ts.net/ws")
            TermField(
                value = url,
                onValueChange = { url = it },
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri),
                modifier = Modifier.fillMaxWidth().padding(vertical = 8.dp),
            )
            Rule()
            FieldLabel("token")
            TermField(
                value = token,
                onValueChange = { token = it },
                mask = true,
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password),
                modifier = Modifier.fillMaxWidth().padding(vertical = 8.dp),
            )
            Rule()

            Row(horizontalArrangement = Arrangement.spacedBy(8.dp), modifier = Modifier.padding(top = 6.dp)) {
                KeyCap(
                    "connect",
                    onClick = {
                        if (Build.VERSION.SDK_INT >= 33) {
                            askNotifications.launch(Manifest.permission.POST_NOTIFICATIONS)
                        } else {
                            connect()
                        }
                    },
                    enabled = url.startsWith("ws") && token.isNotBlank(),
                    modifier = Modifier.weight(1f),
                )
                if (canDismiss) {
                    KeyCap("disconnect", onClick = { CollieService.stop(context) }, modifier = Modifier.weight(1f))
                }
            }

            if (canDismiss) {
                SectionRule("theme", Modifier.padding(top = 18.dp))
                ThemePicker()
            }
        }
    }
}

@Composable
private fun FieldLabel(label: String, hint: String? = null) {
    val t = LocalTerm.current
    Column {
        Text(label, color = t.fg, style = MaterialTheme.typography.bodySmall, fontWeight = FontWeight.Bold)
        hint?.let { Text(it, color = t.dim, style = MaterialTheme.typography.bodySmall) }
    }
}

/** "follow herdr" plus every theme, each row drawn in its own colors. */
@Composable
private fun ThemePicker() {
    val context = LocalContext.current
    val choice by ThemeSettings.choice.collectAsState()
    val live by CollieRepo.herdrTheme.collectAsState()
    val last by ThemeSettings.lastHerdr.collectAsState()
    val herdr = Themes.byId((live ?: last)?.name) ?: Themes.default

    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
        ThemeRow(herdr, "follow herdr · ${herdr.id}", selected = choice == Prefs.FOLLOW_HERDR) {
            ThemeSettings.choose(context, Prefs.FOLLOW_HERDR)
        }
        Themes.all.forEach { theme ->
            ThemeRow(theme, theme.id, selected = choice == theme.id) {
                ThemeSettings.choose(context, theme.id)
            }
        }
    }
}

@Composable
private fun ThemeRow(theme: TermTheme, label: String, selected: Boolean, onClick: () -> Unit) {
    val current = LocalTerm.current
    Row(
        Modifier
            .fillMaxWidth()
            .heightIn(min = 48.dp)
            .border(1.dp, if (selected) current.accent else current.dim.copy(alpha = 0.4f))
            .background(theme.bg)
            .clickable(onClick = onClick)
            .padding(horizontal = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            buildAnnotatedString {
                withStyle(SpanStyle(color = theme.accent)) { append(if (selected) "(•) " else "( ) ") }
                withStyle(SpanStyle(color = theme.fg)) { append(label) }
            },
            style = MaterialTheme.typography.bodyMedium,
            modifier = Modifier.weight(1f),
        )
        // The six hues the terminal uses most.
        Text(
            buildAnnotatedString {
                (1..6).forEach { withStyle(SpanStyle(color = theme.ansi[it])) { append("█") } }
            },
            style = MaterialTheme.typography.bodyMedium,
        )
    }
}
