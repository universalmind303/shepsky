package dev.shepsky.service

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import dev.shepsky.data.Prefs

/**
 * Reconnects after a reboot or an app update, so notifications keep coming
 * without opening the app first. (Allowed for connectedDevice services;
 * Android 15 blocks dataSync ones from boot.)
 */
class BootReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        when (intent.action) {
            Intent.ACTION_BOOT_COMPLETED, Intent.ACTION_MY_PACKAGE_REPLACED ->
                if (Prefs.isPaired(context)) ShepskyService.start(context)
        }
    }
}
