package dev.collie.data

import android.content.Context

/** Pairing details. Plain prefs: this is a personal tool on your own device. */
object Prefs {
    private const val FILE = "collie"

    fun url(context: Context): String =
        prefs(context).getString("url", "") ?: ""

    fun token(context: Context): String =
        prefs(context).getString("token", "") ?: ""

    fun isPaired(context: Context): Boolean =
        url(context).isNotBlank() && token(context).isNotBlank()

    fun save(context: Context, url: String, token: String) {
        prefs(context).edit().putString("url", url.trim()).putString("token", token.trim()).apply()
    }

    private fun prefs(context: Context) =
        context.getSharedPreferences(FILE, Context.MODE_PRIVATE)
}
