package dev.shepsky.ui

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.graphics.luminance

/**
 * A terminal color theme. The terminal mirror uses [ansi], [fg] and [bg]
 * exactly as the theme defines them; the app's chrome (status tags, rules,
 * dim text) derives from them, nudged where needed to stay readable on [bg].
 */
class TermTheme(
    /** herdr's theme name, e.g. "rose-pine". */
    val id: String,
    val dark: Boolean,
    bg: Long,
    fg: Long,
    selection: Long,
    accent: Long,
    ansi: LongArray,
) {
    val bg = Color(bg)
    val fg = Color(fg)
    val selection = Color(selection)

    /** The 16 ANSI colors, untouched (for terminal output). */
    val ansi: List<Color> = ansi.map(::Color)

    /** herdr's accent for this theme. */
    val accent = readable(Color(accent), 3.0)

    /** Secondary text and rules: the theme's "comment" grey when it reads
     *  well, otherwise a fg/bg blend at about 4:1. */
    val dim: Color = this.ansi[8].takeIf { contrast(it, this.bg) in 3.0..7.0 }
        ?: (18 downTo 1).map { lerp(this.fg, this.bg, it / 20f) }.firstOrNull { contrast(it, this.bg) >= 4.0 }
        ?: this.fg

    // Status colors: the theme's own red/green/yellow, readable on bg.
    val red = readable(this.ansi[1], 3.0)
    val green = readable(this.ansi[2], 3.0)
    val yellow = readable(this.ansi[3], 3.0)

    /** [color] pulled toward [fg] until it reaches [min] contrast with [bg]. */
    private fun readable(color: Color, min: Double): Color =
        (0..19).map { lerp(color, fg, it / 20f) }.firstOrNull { contrast(it, bg) >= min } ?: fg
}

private fun contrast(a: Color, b: Color): Double {
    val (hi, lo) = listOf(a.luminance(), b.luminance()).sortedDescending()
    return (hi + 0.05) / (lo + 0.05)
}

/**
 * herdr's built-in themes, so shepsky can look like the herdr it's watching.
 * Terminal palettes are the canonical ones (iTerm2-Color-Schemes, MIT, as
 * bundled with Ghostty); accents are herdr's (src/app/state.rs). "terminal"
 * is a plain dark terminal (herdr's own uses your terminal's colors).
 */
object Themes {
    val all = listOf(
    TermTheme("catppuccin", dark = true, bg = 0xFF1E1E2E, fg = 0xFFCDD6F4, selection = 0xFF585B70, accent = 0xFF89B4FA,
        ansi = longArrayOf(0xFF45475A, 0xFFF38BA8, 0xFFA6E3A1, 0xFFF9E2AF, 0xFF89B4FA, 0xFFF5C2E7, 0xFF94E2D5, 0xFFA6ADC8, 0xFF585B70, 0xFFF37799, 0xFF89D88B, 0xFFEBD391, 0xFF74A8FC, 0xFFF2AEDE, 0xFF6BD7CA, 0xFFBAC2DE)),
    TermTheme("catppuccin-latte", dark = false, bg = 0xFFEFF1F5, fg = 0xFF4C4F69, selection = 0xFFACB0BE, accent = 0xFF1E66F5,
        ansi = longArrayOf(0xFF5C5F77, 0xFFD20F39, 0xFF40A02B, 0xFFDF8E1D, 0xFF1E66F5, 0xFFEA76CB, 0xFF179299, 0xFFACB0BE, 0xFF6C6F85, 0xFFDE293E, 0xFF49AF3D, 0xFFEEA02D, 0xFF456EFF, 0xFFFE85D8, 0xFF2D9FA8, 0xFFBCC0CC)),
    TermTheme("terminal", dark = true, bg = 0xFF14141A, fg = 0xFFD6D6DC, selection = 0xFF3A3D4A, accent = 0xFF2472C8,
        ansi = longArrayOf(0xFF000000, 0xFFCD3131, 0xFF0DBC79, 0xFFE5E510, 0xFF2472C8, 0xFFBC3FBC, 0xFF11A8CD, 0xFFE5E5E5, 0xFF666666, 0xFFF14C4C, 0xFF23D18B, 0xFFF5F543, 0xFF3B8EEA, 0xFFD670D6, 0xFF29B8DB, 0xFFFFFFFF)),
    TermTheme("tokyo-night", dark = true, bg = 0xFF1A1B26, fg = 0xFFC0CAF5, selection = 0xFF33467C, accent = 0xFF7AA2F7,
        ansi = longArrayOf(0xFF15161E, 0xFFF7768E, 0xFF9ECE6A, 0xFFE0AF68, 0xFF7AA2F7, 0xFFBB9AF7, 0xFF7DCFFF, 0xFFA9B1D6, 0xFF414868, 0xFFF7768E, 0xFF9ECE6A, 0xFFE0AF68, 0xFF7AA2F7, 0xFFBB9AF7, 0xFF7DCFFF, 0xFFC0CAF5)),
    TermTheme("tokyo-night-day", dark = false, bg = 0xFFE1E2E7, fg = 0xFF3760BF, selection = 0xFF99A7DF, accent = 0xFF2E7DE9,
        ansi = longArrayOf(0xFFE9E9ED, 0xFFF52A65, 0xFF587539, 0xFF8C6C3E, 0xFF2E7DE9, 0xFF9854F1, 0xFF007197, 0xFF6172B0, 0xFFA1A6C5, 0xFFF52A65, 0xFF587539, 0xFF8C6C3E, 0xFF2E7DE9, 0xFF9854F1, 0xFF007197, 0xFF3760BF)),
    TermTheme("dracula", dark = true, bg = 0xFF282A36, fg = 0xFFF8F8F2, selection = 0xFF44475A, accent = 0xFFBD93F9,
        ansi = longArrayOf(0xFF21222C, 0xFFFF5555, 0xFF50FA7B, 0xFFF1FA8C, 0xFFBD93F9, 0xFFFF79C6, 0xFF8BE9FD, 0xFFF8F8F2, 0xFF6272A4, 0xFFFF6E6E, 0xFF69FF94, 0xFFFFFFA5, 0xFFD6ACFF, 0xFFFF92DF, 0xFFA4FFFF, 0xFFFFFFFF)),
    TermTheme("nord", dark = true, bg = 0xFF2E3440, fg = 0xFFD8DEE9, selection = 0xFFECEFF4, accent = 0xFF88C0D0,
        ansi = longArrayOf(0xFF3B4252, 0xFFBF616A, 0xFFA3BE8C, 0xFFEBCB8B, 0xFF81A1C1, 0xFFB48EAD, 0xFF88C0D0, 0xFFE5E9F0, 0xFF596377, 0xFFBF616A, 0xFFA3BE8C, 0xFFEBCB8B, 0xFF81A1C1, 0xFFB48EAD, 0xFF8FBCBB, 0xFFECEFF4)),
    TermTheme("gruvbox", dark = true, bg = 0xFF282828, fg = 0xFFEBDBB2, selection = 0xFF665C54, accent = 0xFFD79921,
        ansi = longArrayOf(0xFF282828, 0xFFCC241D, 0xFF98971A, 0xFFD79921, 0xFF458588, 0xFFB16286, 0xFF689D6A, 0xFFA89984, 0xFF928374, 0xFFFB4934, 0xFFB8BB26, 0xFFFABD2F, 0xFF83A598, 0xFFD3869B, 0xFF8EC07C, 0xFFEBDBB2)),
    TermTheme("gruvbox-light", dark = false, bg = 0xFFFBF1C7, fg = 0xFF3C3836, selection = 0xFF3C3836, accent = 0xFF076678,
        ansi = longArrayOf(0xFFFBF1C7, 0xFFCC241D, 0xFF98971A, 0xFFD79921, 0xFF458588, 0xFFB16286, 0xFF689D6A, 0xFF7C6F64, 0xFF928374, 0xFF9D0006, 0xFF79740E, 0xFFB57614, 0xFF076678, 0xFF8F3F71, 0xFF427B58, 0xFF3C3836)),
    TermTheme("one-dark", dark = true, bg = 0xFF21252B, fg = 0xFFABB2BF, selection = 0xFF323844, accent = 0xFF61AFEF,
        ansi = longArrayOf(0xFF21252B, 0xFFE06C75, 0xFF98C379, 0xFFE5C07B, 0xFF61AFEF, 0xFFC678DD, 0xFF56B6C2, 0xFFABB2BF, 0xFF767676, 0xFFE06C75, 0xFF98C379, 0xFFE5C07B, 0xFF61AFEF, 0xFFC678DD, 0xFF56B6C2, 0xFFABB2BF)),
    TermTheme("one-light", dark = false, bg = 0xFFF9F9F9, fg = 0xFF2A2C33, selection = 0xFFEDEDED, accent = 0xFF4078F2,
        ansi = longArrayOf(0xFF000000, 0xFFDE3E35, 0xFF3F953A, 0xFFD2B67C, 0xFF2F5AF3, 0xFF950095, 0xFF3F953A, 0xFFBBBBBB, 0xFF000000, 0xFFDE3E35, 0xFF3F953A, 0xFFD2B67C, 0xFF2F5AF3, 0xFFA00095, 0xFF3F953A, 0xFFFFFFFF)),
    TermTheme("solarized", dark = true, bg = 0xFF002B36, fg = 0xFF839496, selection = 0xFF073642, accent = 0xFF268BD2,
        ansi = longArrayOf(0xFF073642, 0xFFDC322F, 0xFF859900, 0xFFB58900, 0xFF268BD2, 0xFFD33682, 0xFF2AA198, 0xFFEEE8D5, 0xFF335E69, 0xFFCB4B16, 0xFF586E75, 0xFF657B83, 0xFF839496, 0xFF6C71C4, 0xFF93A1A1, 0xFFFDF6E3)),
    TermTheme("solarized-light", dark = false, bg = 0xFFFDF6E3, fg = 0xFF657B83, selection = 0xFFEEE8D5, accent = 0xFF268BD2,
        ansi = longArrayOf(0xFF073642, 0xFFDC322F, 0xFF859900, 0xFFB58900, 0xFF268BD2, 0xFFD33682, 0xFF2AA198, 0xFFBBB5A2, 0xFF002B36, 0xFFCB4B16, 0xFF586E75, 0xFF657B83, 0xFF839496, 0xFF6C71C4, 0xFF93A1A1, 0xFFFDF6E3)),
    TermTheme("kanagawa", dark = true, bg = 0xFF1F1F28, fg = 0xFFDCD7BA, selection = 0xFFDCD7BA, accent = 0xFF7E9CD8,
        ansi = longArrayOf(0xFF090618, 0xFFC34043, 0xFF76946A, 0xFFC0A36E, 0xFF7E9CD8, 0xFF957FB8, 0xFF6A9589, 0xFFC8C093, 0xFF727169, 0xFFE82424, 0xFF98BB6C, 0xFFE6C384, 0xFF7FB4CA, 0xFF938AA9, 0xFF7AA89F, 0xFFDCD7BA)),
    TermTheme("kanagawa-lotus", dark = false, bg = 0xFFF2ECBC, fg = 0xFF545464, selection = 0xFF545464, accent = 0xFF4D699B,
        ansi = longArrayOf(0xFF1F1F28, 0xFFC84053, 0xFF6F894E, 0xFF77713F, 0xFF4D699B, 0xFFB35B79, 0xFF597B75, 0xFF545464, 0xFF8A8980, 0xFFD7474B, 0xFF6E915F, 0xFF836F4A, 0xFF6693BF, 0xFF624C83, 0xFF5E857A, 0xFF43436C)),
    TermTheme("rose-pine", dark = true, bg = 0xFF191724, fg = 0xFFE0DEF4, selection = 0xFF403D52, accent = 0xFFC4A7E7,
        ansi = longArrayOf(0xFF26233A, 0xFFEB6F92, 0xFF31748F, 0xFFF6C177, 0xFF9CCFD8, 0xFFC4A7E7, 0xFFEBBCBA, 0xFFE0DEF4, 0xFF6E6A86, 0xFFEB6F92, 0xFF31748F, 0xFFF6C177, 0xFF9CCFD8, 0xFFC4A7E7, 0xFFEBBCBA, 0xFFE0DEF4)),
    TermTheme("rose-pine-dawn", dark = false, bg = 0xFFFAF4ED, fg = 0xFF575279, selection = 0xFFDFDAD9, accent = 0xFF907AA9,
        ansi = longArrayOf(0xFFF2E9E1, 0xFFB4637A, 0xFF286983, 0xFFEA9D34, 0xFF56949F, 0xFF907AA9, 0xFFD7827E, 0xFF575279, 0xFF9893A5, 0xFFB4637A, 0xFF286983, 0xFFEA9D34, 0xFF56949F, 0xFF907AA9, 0xFFD7827E, 0xFF575279)),
    TermTheme("vesper", dark = true, bg = 0xFF101010, fg = 0xFFFFFFFF, selection = 0xFF988049, accent = 0xFFFFC799,
        ansi = longArrayOf(0xFF101010, 0xFFF5A191, 0xFF90B99F, 0xFFE6B99D, 0xFFACA1CF, 0xFFE29ECA, 0xFFEA83A5, 0xFFA0A0A0, 0xFF7E7E7E, 0xFFFF8080, 0xFF99FFE4, 0xFFFFC799, 0xFFB9AEDA, 0xFFECAAD6, 0xFFF591B2, 0xFFFFFFFF)),
    )

    /** herdr's default. */
    val default: TermTheme = all.first()

    fun byId(id: String?): TermTheme? = all.firstOrNull { it.id == id }
}
