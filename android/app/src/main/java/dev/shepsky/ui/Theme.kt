package dev.shepsky.ui

import android.app.Activity
import android.content.Context
import androidx.compose.foundation.IndicationNodeFactory
import androidx.compose.foundation.interaction.InteractionSource
import androidx.compose.foundation.interaction.PressInteraction
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.LocalIndication
import androidx.compose.foundation.shape.CornerSize
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.LocalRippleConfiguration
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Shapes
import androidx.compose.material3.Typography
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.drawscope.ContentDrawScope
import androidx.compose.ui.node.DelegatableNode
import androidx.compose.ui.node.DrawModifierNode
import androidx.compose.ui.node.invalidateDraw
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.Font
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.sp
import androidx.core.view.WindowCompat
import dev.shepsky.R
import dev.shepsky.core.ShepskyRepo
import dev.shepsky.core.HerdrThemeView
import dev.shepsky.data.Prefs
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.launch

/** Fira Code (OFL), bundled so every phone renders the same. */
val Mono = FontFamily(
    Font(R.font.fira_code_regular, FontWeight.Normal),
    Font(R.font.fira_code_bold, FontWeight.Bold),
)

/** The active terminal theme. */
val LocalTerm = staticCompositionLocalOf { Themes.default }

/** Which theme to use: herdr's (default) or a fixed pick. Persisted. */
object ThemeSettings {
    val choice = MutableStateFlow(Prefs.FOLLOW_HERDR)

    /** herdr's theme as last seen, until the bridge says hello. */
    val lastHerdr = MutableStateFlow<HerdrThemeView?>(null)

    fun load(context: Context) {
        choice.value = Prefs.themeChoice(context)
        lastHerdr.value = Prefs.lastHerdrTheme(context)
    }

    fun choose(context: Context, choice: String) {
        Prefs.setThemeChoice(context, choice)
        this.choice.value = choice
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ShepskyTheme(content: @Composable () -> Unit) {
    val context = LocalContext.current
    val choice by ThemeSettings.choice.collectAsState()
    val live by ShepskyRepo.herdrTheme.collectAsState()
    val last by ThemeSettings.lastHerdr.collectAsState()
    val systemDark = isSystemInDarkTheme()

    // Remember herdr's theme so the next launch starts in it.
    LaunchedEffect(live) {
        live?.takeIf { it != last }?.let {
            Prefs.saveHerdrTheme(context, it)
            ThemeSettings.lastHerdr.value = it
        }
    }

    val herdr = live ?: last
    val id = if (choice == Prefs.FOLLOW_HERDR) herdr?.pick(systemDark) else choice
    val theme = Themes.byId(id) ?: Themes.default

    val view = LocalView.current
    if (!view.isInEditMode) {
        SideEffect {
            val window = (view.context as Activity).window
            WindowCompat.getInsetsController(window, view).apply {
                isAppearanceLightStatusBars = !theme.dark
                isAppearanceLightNavigationBars = !theme.dark
            }
        }
    }

    MaterialTheme(colorScheme = theme.colorScheme(), shapes = FlatShapes, typography = MonoType) {
        CompositionLocalProvider(
            LocalTerm provides theme,
            LocalIndication provides FlatIndication(theme.selection),
            LocalRippleConfiguration provides null,
            content = content,
        )
    }
}

// Every Material surface is flat, square and the terminal's background.
private fun TermTheme.colorScheme() = (if (dark) darkColorScheme() else lightColorScheme()).copy(
    primary = accent, onPrimary = bg,
    secondary = accent, onSecondary = bg,
    tertiary = accent, onTertiary = bg,
    background = bg, onBackground = fg,
    surface = bg, onSurface = fg,
    surfaceVariant = bg, onSurfaceVariant = dim,
    surfaceTint = Color.Transparent,
    surfaceBright = bg, surfaceDim = bg,
    surfaceContainerLowest = bg, surfaceContainerLow = bg, surfaceContainer = bg,
    surfaceContainerHigh = bg, surfaceContainerHighest = bg,
    primaryContainer = selection, onPrimaryContainer = fg,
    secondaryContainer = selection, onSecondaryContainer = fg,
    inverseSurface = fg, inverseOnSurface = bg,
    error = red, onError = bg, errorContainer = bg, onErrorContainer = red,
    outline = dim, outlineVariant = dim,
)

private val Square = RoundedCornerShape(CornerSize(0))
private val FlatShapes = Shapes(
    extraSmall = Square, small = Square, medium = Square, large = Square, extraLarge = Square,
)

/** A tight mono scale: 12 / 14 / 16. */
private val MonoType = run {
    fun style(size: Int, weight: FontWeight = FontWeight.Normal) =
        TextStyle(fontFamily = Mono, fontSize = size.sp, lineHeight = (size + 6).sp, fontWeight = weight)
    Typography(
        displayLarge = style(16, FontWeight.Bold), displayMedium = style(16, FontWeight.Bold),
        displaySmall = style(16, FontWeight.Bold),
        headlineLarge = style(16, FontWeight.Bold), headlineMedium = style(16, FontWeight.Bold),
        headlineSmall = style(16, FontWeight.Bold),
        titleLarge = style(16, FontWeight.Bold), titleMedium = style(14, FontWeight.Bold),
        titleSmall = style(14, FontWeight.Bold),
        bodyLarge = style(16), bodyMedium = style(14), bodySmall = style(12),
        labelLarge = style(14), labelMedium = style(12), labelSmall = style(12),
    )
}

/** Pressed = a flat selection-colored block. No ripple, no animation. */
private data class FlatIndication(val color: Color) : IndicationNodeFactory {
    override fun create(interactionSource: InteractionSource): DelegatableNode =
        FlatIndicationNode(interactionSource, color)
}

private class FlatIndicationNode(
    private val source: InteractionSource,
    private val color: Color,
) : androidx.compose.ui.Modifier.Node(), DrawModifierNode {
    private var pressed = false

    override fun onAttach() {
        coroutineScope.launch {
            var presses = 0
            source.interactions.collect {
                when (it) {
                    is PressInteraction.Press -> presses++
                    is PressInteraction.Release, is PressInteraction.Cancel -> presses--
                }
                if ((presses > 0) != pressed) {
                    pressed = presses > 0
                    invalidateDraw()
                }
            }
        }
    }

    override fun ContentDrawScope.draw() {
        if (pressed) drawRect(color)
        drawContent()
    }
}

// --- agent status, herdr-style -------------------------------------------

/** herdr's status symbols. */
fun statusGlyph(status: String): String = when (status) {
    "blocked" -> "×"
    "working" -> "◐"
    "done" -> "✓"
    "idle" -> "○"
    else -> "·"
}

fun TermTheme.statusColor(status: String): Color = when (status) {
    "blocked" -> red
    "done" -> green
    "working" -> yellow
    else -> dim
}

@Composable
fun statusColor(status: String): Color = LocalTerm.current.statusColor(status)

fun statusLabel(status: String): String = when (status) {
    "blocked" -> "needs input"
    "done" -> "finished"
    else -> status
}
