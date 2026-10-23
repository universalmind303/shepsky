package dev.shepsky.service

import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.IBinder
import dev.shepsky.core.ShepskyRepo
import dev.shepsky.core.UserText
import dev.shepsky.data.Prefs
import dev.shepsky.notif.Notifier
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch

/**
 * Foreground service that owns the native connection so notifications keep
 * arriving with the app closed. Lifecycle: started on pairing, app launch,
 * boot and app update ([BootReceiver]); stopped from Settings' Disconnect.
 */
class ShepskyService : Service() {
    private lateinit var scope: CoroutineScope

    override fun onCreate() {
        super.onCreate()
        Notifier.createChannels(this)
        scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
        // Once per service instance (onStartCommand can run many times).
        scope.launch {
            ShepskyRepo.notes.collect { Notifier.postAgentNote(this@ShepskyService, it) }
        }
        scope.launch {
            ShepskyRepo.conn.collect { conn ->
                val text = UserText.connection(conn, Prefs.url(this@ShepskyService)) ?: "Connected to your Mac"
                getSystemService(android.app.NotificationManager::class.java)
                    .notify(Notifier.STATUS_ID, Notifier.statusNotification(this@ShepskyService, text))
            }
        }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        // connectedDevice, not dataSync: on Android 15 dataSync services get
        // 6 hours per day, and this one runs for as long as you're paired.
        startForeground(
            Notifier.STATUS_ID,
            Notifier.statusNotification(this, "Connecting…"),
            ServiceInfo.FOREGROUND_SERVICE_TYPE_CONNECTED_DEVICE,
        )

        val url = Prefs.url(this)
        val token = Prefs.token(this)
        if (url.isBlank() || token.isBlank()) {
            stopSelf()
            return START_NOT_STICKY
        }
        if (!ShepskyRepo.running) {
            ShepskyRepo.start(url, token)
        }
        return START_STICKY
    }

    override fun onDestroy() {
        scope.cancel()
        ShepskyRepo.stop()
        super.onDestroy()
    }

    override fun onBind(intent: Intent?): IBinder? = null

    companion object {
        fun start(context: Context) {
            context.startForegroundService(Intent(context, ShepskyService::class.java))
        }

        fun stop(context: Context) {
            context.stopService(Intent(context, ShepskyService::class.java))
        }
    }
}
