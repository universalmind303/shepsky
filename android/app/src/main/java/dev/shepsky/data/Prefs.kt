package dev.shepsky.data

import android.content.Context
import dev.shepsky.core.HerdrThemeView

/** Pairing details and look. Plain prefs: this is a personal tool on your own device. */
object Prefs {
    private const val FILE = "shepsky"

    /** Theme choice meaning "whatever herdr on the Mac uses". */
    const val FOLLOW_HERDR = "herdr"

    fun url(context: Context): String =
        prefs(context).getString("url", "") ?: ""

    fun token(context: Context): String =
        prefs(context).getString("token", "") ?: ""

    fun isPaired(context: Context): Boolean =
        url(context).isNotBlank() && token(context).isNotBlank()

    fun save(context: Context, url: String, token: String) {
        prefs(context).edit().putString("url", url.trim()).putString("token", token.trim()).apply()
    }

    /** [FOLLOW_HERDR] or a theme id. */
    fun themeChoice(context: Context): String =
        prefs(context).getString("theme", FOLLOW_HERDR) ?: FOLLOW_HERDR

    fun setThemeChoice(context: Context, choice: String) {
        prefs(context).edit().putString("theme", choice).apply()
    }

    /** herdr's theme as last seen, so the app starts in it before connecting. */
    fun lastHerdrTheme(context: Context): HerdrThemeView? {
        val name = prefs(context).getString("herdr_theme", null) ?: return null
        return HerdrThemeView(
            name = name,
            dark = prefs(context).getString("herdr_theme_dark", null),
            light = prefs(context).getString("herdr_theme_light", null),
        )
    }

    fun saveHerdrTheme(context: Context, theme: HerdrThemeView) {
        prefs(context).edit()
            .putString("herdr_theme", theme.name)
            .putString("herdr_theme_dark", theme.dark)
            .putString("herdr_theme_light", theme.light)
            .apply()
    }

    private fun prefs(context: Context) =
        context.getSharedPreferences(FILE, Context.MODE_PRIVATE)
}
