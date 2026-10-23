package dev.collie

import android.content.Intent
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import dev.collie.ui.App
import dev.collie.ui.CollieTheme
import kotlinx.coroutines.flow.MutableStateFlow

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        pendingPane.value = intent.getStringExtra(EXTRA_PANE_ID)
        setContent {
            CollieTheme {
                App()
            }
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        pendingPane.value = intent.getStringExtra(EXTRA_PANE_ID)
    }

    companion object {
        const val EXTRA_PANE_ID = "pane_id"

        /** Pane a notification tap wants opened; consumed by [App]. */
        val pendingPane = MutableStateFlow<String?>(null)
    }
}
