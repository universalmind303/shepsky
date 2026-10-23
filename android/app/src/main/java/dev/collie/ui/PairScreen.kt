package dev.collie.ui

import android.Manifest
import android.os.Build
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import dev.collie.PairLink
import dev.collie.data.Prefs
import dev.collie.service.CollieService

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun PairScreen(
    prefill: PairLink?,
    canDismiss: Boolean,
    onDismiss: () -> Unit,
    onPaired: () -> Unit,
) {
    val context = LocalContext.current
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
            TopAppBar(
                title = { Text(if (canDismiss) "Settings" else "Pair with your bridge") },
                navigationIcon = {
                    if (canDismiss) {
                        IconButton(onClick = onDismiss) {
                            Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
                        }
                    }
                },
            )
        },
    ) { padding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(padding)
                .padding(horizontal = 20.dp)
                .verticalScroll(rememberScrollState()),
            verticalArrangement = Arrangement.spacedBy(14.dp),
        ) {
            Text(
                buildAnnotatedString {
                    append("On your Mac, run ")
                    withStyle(SpanStyle(fontFamily = FontFamily.Monospace)) { append("collie-bridge pair") }
                    append(" and enter the URL and token it prints.")
                },
                style = MaterialTheme.typography.bodyMedium,
            )
            if (prefill != null) {
                Surface(
                    color = MaterialTheme.colorScheme.secondaryContainer,
                    shape = MaterialTheme.shapes.medium,
                ) {
                    Text(
                        "Filled in from a pairing link. Check that the URL is your Mac, then tap Connect.",
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSecondaryContainer,
                        modifier = Modifier.padding(12.dp),
                    )
                }
            }
            OutlinedTextField(
                value = url,
                onValueChange = { url = it },
                label = { Text("Bridge URL") },
                placeholder = { Text("ws://10.0.2.2:7717/ws") },
                supportingText = { Text("Emulator: ws://10.0.2.2:7717/ws · Tailscale: wss://<mac>.<tailnet>.ts.net/ws") },
                singleLine = true,
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri),
                textStyle = MaterialTheme.typography.bodyMedium.copy(fontFamily = FontFamily.Monospace),
                modifier = Modifier.fillMaxWidth(),
            )
            OutlinedTextField(
                value = token,
                onValueChange = { token = it },
                label = { Text("Token") },
                singleLine = true,
                textStyle = MaterialTheme.typography.bodyMedium.copy(fontFamily = FontFamily.Monospace),
                modifier = Modifier.fillMaxWidth(),
            )
            Button(
                onClick = {
                    if (Build.VERSION.SDK_INT >= 33) {
                        askNotifications.launch(Manifest.permission.POST_NOTIFICATIONS)
                    } else {
                        connect()
                    }
                },
                enabled = url.startsWith("ws") && token.isNotBlank(),
                modifier = Modifier.fillMaxWidth(),
            ) {
                Text("Connect")
            }
            if (canDismiss) {
                OutlinedButton(
                    onClick = { CollieService.stop(context) },
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Text("Disconnect")
                }
            }
        }
    }
}
