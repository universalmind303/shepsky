package dev.collie.service

import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.IBinder
import dev.collie.core.CollieRepo
import dev.collie.core.ConnState
import dev.collie.data.Prefs
import dev.collie.notif.Notifier
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch

/**
 * Foreground service that owns the native connection so notifications keep
 * arriving with the app closed. Lifecycle: started on pairing / app launch,
 * stopped from the pairing screen's Disconnect.
 */
class CollieService : Service() {
    private lateinit var scope: CoroutineScope

    override fun onCreate() {
        super.onCreate()
        Notifier.createChannels(this)
        scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        startForeground(
            Notifier.STATUS_ID,
            Notifier.statusNotification(this, "Connecting…"),
            ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC,
        )

        val url = Prefs.url(this)
        val token = Prefs.token(this)
        if (url.isBlank() || token.isBlank()) {
            stopSelf()
            return START_NOT_STICKY
        }
        if (!CollieRepo.running) {
            CollieRepo.start(url, token)
        }

        scope.launch {
            CollieRepo.notes.collect { Notifier.postAgentNote(this@CollieService, it) }
        }
        scope.launch {
            CollieRepo.conn.collect { conn ->
                val text = when (conn) {
                    ConnState.Idle, ConnState.Connecting -> "Connecting…"
                    ConnState.Connected -> "Connected to the bridge"
                    is ConnState.Disconnected ->
                        "Reconnecting…" + (conn.detail?.let { " ($it)" } ?: "")
                }
                getSystemService(android.app.NotificationManager::class.java)
                    .notify(Notifier.STATUS_ID, Notifier.statusNotification(this@CollieService, text))
            }
        }
        return START_STICKY
    }

    override fun onDestroy() {
        scope.cancel()
        CollieRepo.stop()
        super.onDestroy()
    }

    override fun onBind(intent: Intent?): IBinder? = null

    companion object {
        fun start(context: Context) {
            context.startForegroundService(Intent(context, CollieService::class.java))
        }

        fun stop(context: Context) {
            context.stopService(Intent(context, CollieService::class.java))
        }
    }
}
