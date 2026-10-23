package dev.collie.ui

import androidx.compose.foundation.layout.Column
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
import dev.collie.core.Span

/** Styled terminal rows, unwrapped (callers decide how to scroll or clip). */
@Composable
fun TerminalLines(
    lines: List<List<Span>>,
    modifier: Modifier = Modifier,
    fontSize: TextUnit = 10.sp,
) {
    Column(modifier) {
        lines.forEach { line ->
            Text(
                text = line.toAnnotated(),
                fontFamily = Mono,
                fontSize = fontSize,
                lineHeight = fontSize * 1.25f,
                softWrap = false,
                color = LocalTerm.current.fg,
            )
        }
    }
}

fun List<Span>.isBlankLine(): Boolean = all { it.t.isBlank() && it.bg == null }

private fun List<Span>.toAnnotated(): AnnotatedString = buildAnnotatedString {
    for (span in this@toAnnotated) {
        withStyle(
            SpanStyle(
                color = span.fg.toComposeColor() ?: Color.Unspecified,
                background = span.bg.toComposeColor() ?: Color.Unspecified,
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

private fun String?.toComposeColor(): Color? = this?.let {
    runCatching { Color(android.graphics.Color.parseColor(it)) }.getOrNull()
}
