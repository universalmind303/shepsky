package dev.collie.ui

import android.os.Build
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext

@Composable
fun CollieTheme(content: @Composable () -> Unit) {
    val dark = isSystemInDarkTheme()
    val context = LocalContext.current
    val scheme = when {
        Build.VERSION.SDK_INT >= Build.VERSION_CODES.S ->
            if (dark) dynamicDarkColorScheme(context) else dynamicLightColorScheme(context)
        dark -> darkColorScheme()
        else -> lightColorScheme()
    }
    MaterialTheme(colorScheme = scheme, content = content)
}

/** Shared status palette (matches the bridge's terminal UI). */
fun statusColor(status: String): Color = when (status) {
    "blocked" -> Color(0xFFF14C4C)
    "done" -> Color(0xFF23D18B)
    "working" -> Color(0xFFE5C22E)
    else -> Color(0xFF8A8F98) // idle / unknown
}

fun statusLabel(status: String): String = when (status) {
    "blocked" -> "needs input"
    "done" -> "finished"
    else -> status
}
