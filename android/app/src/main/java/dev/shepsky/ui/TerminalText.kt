package dev.shepsky.ui

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.background
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.rememberScrollState
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.TextUnit
import androidx.compose.ui.unit.sp
import dev.shepsky.core.Span
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.intOrNull

/** Styled terminal rows, unwrapped (callers decide how to scroll or clip). */
@Composable
fun TerminalLines(
    lines: List<List<Span>>,
    modifier: Modifier = Modifier,
    fontSize: TextUnit = 10.sp,
) {
    val theme = LocalTerm.current
    Column(modifier) {
        lines.forEach { line ->
            Text(
                text = line.toAnnotated(theme),
                fontFamily = Mono,
                fontSize = fontSize,
                lineHeight = fontSize * 1.25f,
                softWrap = false,
                color = theme.fg,
            )
        }
    }
}

/**
 * [TerminalLines] that scroll sideways, with a `›` on the right edge of every
 * row that runs past it (like vim's `extends`), so a clipped frame says so.
 */
@Composable
fun ScrollingTerminalLines(lines: List<List<Span>>, fontSize: TextUnit = 10.sp) {
    val theme = LocalTerm.current
    val scroll = rememberScrollState()
    val measurer = rememberTextMeasurer()
    val cell = remember(fontSize) {
        measurer.measure("0", TextStyle(fontFamily = Mono, fontSize = fontSize)).size.width
    }
    BoxWithConstraints(Modifier.fillMaxWidth()) {
        TerminalLines(lines, Modifier.horizontalScroll(scroll), fontSize)
        val visible = constraints.maxWidth
        Column(Modifier.align(Alignment.TopEnd)) {
            lines.forEach { line ->
                val clipped = line.sumOf { it.t.length } * cell - scroll.value > visible
                Text(
                    if (clipped) "›" else " ",
                    fontFamily = Mono,
                    fontSize = fontSize,
                    lineHeight = fontSize * 1.25f,
                    softWrap = false,
                    color = theme.accent,
                    modifier = if (clipped) Modifier.background(theme.bg) else Modifier,
                )
            }
        }
    }
}

fun List<Span>.isBlankLine(): Boolean = all { it.t.isBlank() && it.bg == null && !it.r }

private fun List<Span>.toAnnotated(theme: TermTheme): AnnotatedString = buildAnnotatedString {
    for (span in this@toAnnotated) {
        var fg = span.fg.toColor(theme)
        var bg = span.bg.toColor(theme)
        if (span.r) {
            // Reverse video swaps the resolved colors, defaults included.
            val swappedFg = bg ?: theme.bg
            bg = fg ?: theme.fg
            fg = swappedFg
        }
        withStyle(
            SpanStyle(
                color = fg ?: Color.Unspecified,
                background = bg ?: Color.Unspecified,
                fontWeight = if (span.b) FontWeight.Bold else null,
                fontStyle = if (span.i) FontStyle.Italic else null,
                textDecoration = if (span.u) TextDecoration.Underline else null,
            )
        ) {
            append(span.t)
        }
    }
    if (this@toAnnotated.isEmpty()) append(" ")
}

/** ANSI 0–15 from the theme; `#rrggbb` as is; null = the theme default. */
private fun JsonPrimitive?.toColor(theme: TermTheme): Color? = when {
    this == null -> null
    isString -> runCatching { Color(android.graphics.Color.parseColor(content)) }.getOrNull()
    else -> intOrNull?.let { theme.ansi.getOrNull(it) }
}
