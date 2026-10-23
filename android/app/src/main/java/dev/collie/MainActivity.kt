package dev.collie

import android.content.Intent
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import dev.collie.ui.App
import dev.collie.ui.CollieTheme
import dev.collie.ui.ThemeSettings
import kotlinx.coroutines.flow.MutableStateFlow

/** Pairing details from a `collie://pair?url=…&token=…` link. */
data class PairLink(val url: String, val token: String)

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        ThemeSettings.load(this)
        // Only on a fresh start: a recreated activity (rotation…) would
        // otherwise replay the notification tap or pairing link.
        if (savedInstanceState == null) handle(intent)
        setContent {
            CollieTheme {
                App()
            }
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        handle(intent)
    }

    private fun handle(intent: Intent) {
        intent.getStringExtra(EXTRA_PANE_ID)?.let { pendingPane.value = it }
        val data = intent.data
        if (intent.action == Intent.ACTION_VIEW && data?.scheme == "collie" && data.host == "pair") {
            pendingPair.value = PairLink(
                url = data.getQueryParameter("url").orEmpty(),
                token = data.getQueryParameter("token").orEmpty(),
            )
        }
    }

    companion object {
        const val EXTRA_PANE_ID = "pane_id"

        /** Pane a notification tap wants opened; consumed by [App]. */
        val pendingPane = MutableStateFlow<String?>(null)

        /** Pairing link to pre-fill; consumed by [App]. */
        val pendingPair = MutableStateFlow<PairLink?>(null)
    }
}
