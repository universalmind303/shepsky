package dev.shepsky.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.text.selection.LocalTextSelectionColors
import androidx.compose.foundation.text.selection.TextSelectionColors
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.delay

/** One status-line segment. */
data class Seg(val text: String, val color: Color? = null, val bold: Boolean = false)

/** `a · b · c`, tmux status-line style. */
@Composable
fun segments(vararg segs: Seg): AnnotatedString {
    val t = LocalTerm.current
    return buildAnnotatedString {
        segs.forEachIndexed { i, seg ->
            if (i > 0) withStyle(SpanStyle(color = t.dim)) { append(" · ") }
            withStyle(SpanStyle(color = seg.color ?: t.fg, fontWeight = if (seg.bold) FontWeight.Bold else null)) {
                append(seg.text)
            }
        }
    }
}

/** The top bar: one status line over a rule. */
@Composable
fun TopLine(
    line: AnnotatedString,
    onBack: (() -> Unit)? = null,
    actions: @Composable RowScope.() -> Unit = {},
) {
    val t = LocalTerm.current
    Column(Modifier.background(t.bg).statusBarsPadding()) {
        Row(Modifier.fillMaxWidth().height(56.dp), verticalAlignment = Alignment.CenterVertically) {
            if (onBack != null) TapText("←", onBack, contentDescription = "Back") else Spacer(Modifier.width(16.dp))
            Text(
                line,
                style = MaterialTheme.typography.bodyMedium,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f),
            )
            actions()
        }
        Rule()
    }
}

/** A bare text button with a full-size touch target, e.g. `[≡]`. */
@Composable
fun TapText(
    text: String,
    onClick: () -> Unit,
    contentDescription: String? = null,
    color: Color = LocalTerm.current.fg,
    enabled: Boolean = true,
) {
    Box(
        Modifier
            .heightIn(min = 48.dp)
            .widthIn(min = 48.dp)
            .clickable(enabled = enabled, onClick = onClick)
            .semantics { contentDescription?.let { this.contentDescription = it } }
            .padding(horizontal = 12.dp),
        contentAlignment = Alignment.Center,
    ) {
        Text(text, color = color, style = MaterialTheme.typography.bodyMedium)
    }
}

/** A 1px rule in the theme's dim color. */
@Composable
fun Rule(color: Color = LocalTerm.current.dim) {
    val px = with(LocalDensity.current) { 1.toDp() }
    Box(Modifier.fillMaxWidth().height(px).background(color))
}

/** `── label ──────────` */
@Composable
fun SectionRule(label: String, modifier: Modifier = Modifier, color: Color = LocalTerm.current.dim) {
    Text(
        "── $label " + "─".repeat(160),
        color = color,
        style = MaterialTheme.typography.bodySmall,
        maxLines = 1,
        softWrap = false,
        overflow = TextOverflow.Clip,
        modifier = modifier.fillMaxWidth(),
    )
}

/** A key: square, 1px border, at least 48dp. [hot] = lit up in red;
 *  [selected] = the chosen one of a set, in the accent color. */
@Composable
fun KeyCap(
    label: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    hot: Boolean = false,
    selected: Boolean = false,
    contentDescription: String? = null,
) {
    val t = LocalTerm.current
    val border = when {
        !enabled -> t.dim.copy(alpha = 0.5f)
        hot -> t.red
        selected -> t.accent
        else -> t.dim
    }
    val text = when {
        !enabled -> t.dim.copy(alpha = 0.7f)
        hot -> t.bg
        selected -> t.accent
        else -> t.fg
    }
    Box(
        modifier
            .heightIn(min = 52.dp)
            .widthIn(min = 48.dp)
            .background(if (hot && enabled) t.red else Color.Transparent)
            .border(1.dp, border)
            .clickable(enabled = enabled, onClick = onClick)
            .semantics { contentDescription?.let { this.contentDescription = it } }
            .padding(horizontal = 10.dp),
        contentAlignment = Alignment.Center,
    ) {
        Text(
            label,
            color = text,
            style = MaterialTheme.typography.bodyMedium,
            fontWeight = if ((hot || selected) && enabled) FontWeight.Bold else null,
        )
    }
}

/** `[×] needs input` in the status color. */
@Composable
fun statusTag(status: String, withLabel: Boolean = true): AnnotatedString {
    val color = statusColor(status)
    return buildAnnotatedString {
        withStyle(SpanStyle(color = color)) {
            append("[${statusGlyph(status)}]")
            if (withLabel) append(" ${statusLabel(status)}")
        }
    }
}

/**
 * A one-line terminal input: `> text█`. The block cursor blinks while the
 * field has focus (the one bit of motion in the app). External changes to
 * [value] (cleared, restored) put the cursor at the end.
 */
@Composable
fun TermField(
    value: String,
    onValueChange: (String) -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    placeholder: String = "",
    prompt: String = ">",
    mask: Boolean = false,
    maxLines: Int = 1,
    keyboardOptions: KeyboardOptions = KeyboardOptions.Default,
    /** Space around the text that still counts as the field: any tap in it
     *  focuses the field and opens the keyboard. */
    contentPadding: PaddingValues = PaddingValues(vertical = 14.dp),
) {
    val t = LocalTerm.current
    val style = MaterialTheme.typography.bodyMedium.copy(color = if (enabled) t.fg else t.dim)
    var field by remember { mutableStateOf(TextFieldValue(value, TextRange(value.length))) }
    val shown = if (field.text == value) field else TextFieldValue(value, TextRange(value.length))
    var focused by remember { mutableStateOf(false) }
    var blinkOn by remember { mutableStateOf(true) }
    var layout by remember { mutableStateOf<TextLayoutResult?>(null) }
    val measurer = rememberTextMeasurer()
    val cell = remember(style) { measurer.measure("0", style).size }

    LaunchedEffect(focused, shown.text) {
        blinkOn = true
        while (focused) {
            delay(530)
            blinkOn = !blinkOn
        }
    }

    CompositionLocalProvider(LocalTextSelectionColors provides TextSelectionColors(t.accent, t.selection)) {
        BasicTextField(
            value = shown,
            onValueChange = {
                field = it
                if (it.text != value) onValueChange(it.text)
            },
            enabled = enabled,
            textStyle = style,
            cursorBrush = SolidColor(Color.Transparent), // we draw a block instead
            singleLine = maxLines == 1,
            maxLines = maxLines,
            keyboardOptions = keyboardOptions,
            visualTransformation = if (mask && !focused) PasswordVisualTransformation('•') else VisualTransformation.None,
            onTextLayout = { layout = it },
            modifier = modifier.onFocusChanged { focused = it.isFocused },
            decorationBox = { inner ->
                Row(Modifier.padding(contentPadding), verticalAlignment = Alignment.Top) {
                    Text("$prompt ", style = style.copy(color = if (enabled) t.accent else t.dim))
                    Box(
                        Modifier
                            .weight(1f)
                            .drawWithContent {
                                drawContent()
                                val l = layout
                                if (focused && blinkOn && l != null && shown.selection.collapsed) {
                                    val at = l.getCursorRect(shown.selection.start.coerceIn(0, shown.text.length))
                                    drawRect(
                                        t.fg.copy(alpha = 0.75f),
                                        topLeft = Offset(at.left, at.top),
                                        size = Size(cell.width.toFloat(), at.height),
                                    )
                                }
                            },
                    ) {
                        if (shown.text.isEmpty() && !focused) {
                            Text(placeholder, style = style.copy(color = t.dim), maxLines = 1, overflow = TextOverflow.Ellipsis)
                        }
                        inner()
                    }
                }
            },
        )
    }
}

/** Messages as an inverse block, like a tmux message line. */
@Composable
fun TermSnackbarHost(state: SnackbarHostState) {
    val t = LocalTerm.current
    SnackbarHost(state) { data ->
        Box(
            Modifier
                .padding(horizontal = 16.dp, vertical = 8.dp)
                .fillMaxWidth()
                .background(t.fg)
                .padding(horizontal = 12.dp, vertical = 12.dp),
        ) {
            Text(data.visuals.message, color = t.bg, style = MaterialTheme.typography.bodyMedium)
        }
    }
}

/** A full-width alert line in inverse video, with an optional action. */
@Composable
fun AlertLine(text: String, color: Color = LocalTerm.current.red, action: Pair<String, () -> Unit>? = null) {
    val t = LocalTerm.current
    Row(
        Modifier.fillMaxWidth().background(color).padding(start = 16.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            "! $text",
            color = t.bg,
            style = MaterialTheme.typography.bodySmall,
            fontWeight = FontWeight.Bold,
            modifier = Modifier.weight(1f).padding(vertical = 8.dp),
        )
        action?.let { (label, onClick) -> TapText("[$label]", onClick, color = t.bg) }
    }
}
