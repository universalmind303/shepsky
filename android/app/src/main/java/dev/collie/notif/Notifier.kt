package dev.collie.notif

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import dev.collie.MainActivity
import dev.collie.R
import dev.collie.core.NoteView

object Notifier {
    const val CHANNEL_STATUS = "collie_status"
    const val CHANNEL_NEEDS_INPUT = "collie_needs_input"
    const val CHANNEL_FINISHED = "collie_finished"
    const val STATUS_ID = 1

    fun createChannels(context: Context) {
        val manager = context.getSystemService(NotificationManager::class.java)
        manager.createNotificationChannel(
            NotificationChannel(CHANNEL_STATUS, "Connection status", NotificationManager.IMPORTANCE_MIN)
        )
        manager.createNotificationChannel(
            NotificationChannel(CHANNEL_NEEDS_INPUT, "Agent needs input", NotificationManager.IMPORTANCE_HIGH)
        )
        manager.createNotificationChannel(
            NotificationChannel(CHANNEL_FINISHED, "Agent finished", NotificationManager.IMPORTANCE_DEFAULT)
        )
    }

    /** The persistent foreground-service notification. */
    fun statusNotification(context: Context, text: String): Notification =
        Notification.Builder(context, CHANNEL_STATUS)
            .setSmallIcon(R.drawable.ic_launcher_foreground)
            .setContentTitle("collie")
            .setContentText(text)
            .setContentIntent(openApp(context, null))
            .setOngoing(true)
            .build()

    fun postAgentNote(context: Context, note: NoteView) {
        if (context.checkSelfPermission(android.Manifest.permission.POST_NOTIFICATIONS)
            != PackageManager.PERMISSION_GRANTED
        ) {
            return
        }
        val needsInput = note.kind == "needs_input"
        val channel = if (needsInput) CHANNEL_NEEDS_INPUT else CHANNEL_FINISHED
        val verb = if (needsInput) "needs input" else "finished"
        val where = note.workspace_label?.let { " · $it" } ?: ""
        val notification = Notification.Builder(context, channel)
            .setSmallIcon(R.drawable.ic_launcher_foreground)
            .setContentTitle("${note.agent_label} $verb$where")
            .setContentText(note.title ?: "")
            .setContentIntent(openApp(context, note.pane_id))
            .setAutoCancel(true)
            .build()
        // One slot per pane: a later "finished" replaces its "needs input".
        context.getSystemService(NotificationManager::class.java)
            .notify(note.pane_id.hashCode(), notification)
    }

    private fun openApp(context: Context, paneId: String?): PendingIntent {
        val intent = Intent(context, MainActivity::class.java).apply {
            addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP)
            paneId?.let { putExtra(MainActivity.EXTRA_PANE_ID, it) }
        }
        return PendingIntent.getActivity(
            context,
            paneId?.hashCode() ?: 0,
            intent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
    }
}
