package dev.shepsky.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextIndent
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.em
import androidx.compose.ui.unit.sp
import dev.shepsky.core.ChatItem
import dev.shepsky.core.ChatState
import dev.shepsky.core.Frame

/** Lines of the live screen shown while an agent waits on you. */
private const val QUESTION_LINES = 14

/**
 * An agent's conversation, newest at the bottom. Follows new items while
 * you're at the bottom; scrolling up stops that ([↓ latest] resumes) and,
 * near the top, loads older pages. While the agent waits on you, the end of
 * its live screen (the question and its choices) sits at the bottom.
 */
@Composable
fun ChatView(
    chat: ChatState,
    blocked: Boolean,
    frame: Frame?,
    onLoadOlder: () -> Unit,
    onShowTerminal: () -> Unit,
) {
    val t = LocalTerm.current
    if (!chat.loaded) {
        Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) { BlinkingLine("reading the conversation") }
        return
    }
    if (chat.unavailable != null && chat.items.isEmpty()) {
        Column(
            Modifier.fillMaxSize().padding(24.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp, Alignment.CenterVertically),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Text("No conversation to show: ${chat.unavailable}.", color = t.dim, style = MaterialTheme.typography.bodyMedium)
            KeyCap("show terminal", onClick = onShowTerminal)
        }
        return
    }

    val list = rememberLazyListState()
    var following by remember { mutableStateOf(true) }
    val slack = with(LocalDensity.current) { 32.dp.toPx() }
    LaunchedEffect(list) {
        var wasScrolling = false
        snapshotFlow { list.isScrollInProgress }.collect { scrolling ->
            if (wasScrolling && !scrolling) {
                following = list.firstVisibleItemIndex == 0 && list.firstVisibleItemScrollOffset < slack
            }
            wasScrolling = scrolling
        }
    }
    LaunchedEffect(chat.items.lastOrNull()?.id, blocked) {
        if (following) list.scrollToItem(0)
    }

    Box(Modifier.fillMaxSize()) {
        // Reversed: item 0 is the bottom, so the newest stays in view.
        LazyColumn(
            state = list,
            reverseLayout = true,
            contentPadding = PaddingValues(horizontal = 14.dp, vertical = 12.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp),
            modifier = Modifier.fillMaxSize(),
        ) {
            if (blocked) item(key = "question") { WaitingCard(frame, onShowTerminal) }
            val newestFirst = chat.items.asReversed()
            items(newestFirst.size, key = { newestFirst[it].id }) { ChatRow(newestFirst[it]) }
            item(key = "top") {
                if (chat.hasOlder) {
                    // Coming into view near the top: fetch the next page.
                    LaunchedEffect(chat.items.firstOrNull()?.id) { onLoadOlder() }
                    Notice("loading older…")
                } else {
                    Notice("start of conversation")
                }
            }
        }
        if (!following) {
            KeyCap(
                "↓ latest",
                onClick = { following = true },
                modifier = Modifier.align(Alignment.BottomEnd).padding(12.dp).background(t.bg),
            )
        }
    }
}

@Composable
private fun ChatRow(item: ChatItem) {
    when (item.kind) {
        "prompt" -> PromptRow(item.text)
        "reply" -> Text(markdownLite(item.text), style = MaterialTheme.typography.bodyMedium)
        "thinking" -> ThinkingRow(item.text)
        "tool" -> ToolRow(item)
        "question" -> QuestionRow(item)
        "notice" -> Notice(item.text)
        else -> {} // newer bridge, older app
    }
}

/** `> what you asked`, on a faint band like a terminal selection. */
@Composable
private fun PromptRow(text: String) {
    val t = LocalTerm.current
    Text(
        buildAnnotatedString {
            withStyle(SpanStyle(color = t.accent, fontWeight = FontWeight.Bold)) { append("> ") }
            append(text)
        },
        style = MaterialTheme.typography.bodyMedium,
        modifier = Modifier.fillMaxWidth().background(t.selection.copy(alpha = 0.45f)).padding(horizontal = 8.dp, vertical = 6.dp),
    )
}

/** `∴ thinking`, tap to read it (when the agent shares it). */
@Composable
private fun ThinkingRow(text: String) {
    val t = LocalTerm.current
    var open by rememberSaveable { mutableStateOf(false) }
    val canOpen = text.isNotBlank()
    Column(
        Modifier.fillMaxWidth().clickable(enabled = canOpen) { open = !open },
    ) {
        Text(
            when {
                !canOpen -> "∴ thought"
                open -> "∴ thinking ▾"
                else -> "∴ thinking ▸"
            },
            color = t.dim,
            fontStyle = FontStyle.Italic,
            style = MaterialTheme.typography.bodySmall,
        )
        if (open) {
            Text(
                text,
                color = t.dim,
                style = MaterialTheme.typography.bodySmall,
                modifier = Modifier.padding(start = 16.dp, top = 4.dp),
            )
        }
    }
}

/**
 * `● Bash  $ npm test` and the first line of what came back; tap for the
 * full input and output. The dot is the tool's state.
 */
@Composable
private fun ToolRow(item: ChatItem) {
    val t = LocalTerm.current
    var open by rememberSaveable(item.id) { mutableStateOf(false) }
    val dot = when (item.status) {
        "ok" -> t.green
        "error" -> t.red
        else -> t.yellow
    }
    val small = MaterialTheme.typography.bodySmall
    Column(Modifier.fillMaxWidth().clickable { open = !open }) {
        Text(
            buildAnnotatedString {
                withStyle(SpanStyle(color = dot)) { append("● ") }
                withStyle(SpanStyle(fontWeight = FontWeight.Bold)) { append(item.name) }
                if (item.summary.isNotBlank()) append("  ${item.summary}")
            },
            // Wrapped lines hang under the name, not the dot.
            style = MaterialTheme.typography.bodyMedium.copy(textIndent = TextIndent(restLine = 2.em)),
            maxLines = if (open) Int.MAX_VALUE else 2,
            overflow = TextOverflow.Ellipsis,
        )
        val output = item.output?.trim().orEmpty()
        if (!open) {
            val first = when {
                item.status == "running" -> "running…"
                output.isNotEmpty() -> output.lineSequence().first() + if ('\n' in output) " …" else ""
                else -> null
            }
            first?.let {
                Text(
                    "  ⎿ $it",
                    color = if (item.status == "error") t.red else t.dim,
                    style = small,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        } else {
            item.detail?.let { Block(it, t.dim) }
            if (output.isNotEmpty()) Block(output, if (item.status == "error") t.red else t.fg)
        }
    }
}

/** Preformatted text, scrolled sideways rather than wrapped. */
@Composable
private fun Block(text: String, color: androidx.compose.ui.graphics.Color) {
    val t = LocalTerm.current
    Text(
        text,
        color = color,
        fontSize = 12.sp,
        lineHeight = 16.sp,
        fontFamily = Mono,
        softWrap = false,
        modifier = Modifier
            .padding(start = 16.dp, top = 6.dp)
            .fillMaxWidth()
            .background(t.selection.copy(alpha = 0.25f))
            .horizontalScroll(rememberScrollState())
            .padding(8.dp),
    )
}

/** The agent asking you to choose; answer with the number keys. */
@Composable
private fun QuestionRow(item: ChatItem) {
    val t = LocalTerm.current
    Column(
        Modifier.fillMaxWidth().border(1.dp, if (item.answer == null) t.accent else t.dim).padding(10.dp),
        verticalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        item.questions.forEach { q ->
            q.header?.let { Text(it, color = t.dim, style = MaterialTheme.typography.bodySmall) }
            Text(q.question, fontWeight = FontWeight.Bold, style = MaterialTheme.typography.bodyMedium)
            q.options.forEachIndexed { i, option ->
                Text(
                    buildAnnotatedString {
                        withStyle(SpanStyle(color = t.accent)) { append("${i + 1}. ") }
                        append(option)
                    },
                    style = MaterialTheme.typography.bodyMedium,
                )
            }
        }
        item.answer?.let { Text("⎿ $it", color = t.dim, style = MaterialTheme.typography.bodySmall) }
    }
}

/**
 * While the agent waits on you: the end of its live screen, where its
 * question and choices are, exactly as it drew them. Answer with the keys
 * below; tap for the whole terminal.
 */
@Composable
private fun WaitingCard(frame: Frame?, onShowTerminal: () -> Unit) {
    val t = LocalTerm.current
    val lines = frame?.lines.orEmpty().dropLastWhile { line -> line.all { it.t.isBlank() } }.takeLast(QUESTION_LINES)
    Column(
        Modifier.fillMaxWidth().border(1.dp, t.red).clickable(onClick = onShowTerminal).padding(10.dp),
        verticalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        Text(statusTag("blocked"), style = MaterialTheme.typography.bodySmall, fontWeight = FontWeight.Bold)
        if (lines.isEmpty()) {
            BlinkingLine("reading its screen")
        } else {
            TerminalLines(lines, fontSize = 11.sp, modifier = Modifier.horizontalScroll(rememberScrollState()))
        }
    }
}

@Composable
private fun Notice(text: String) {
    val t = LocalTerm.current
    Text(
        "── $text ──",
        color = t.dim,
        style = MaterialTheme.typography.bodySmall,
        textAlign = TextAlign.Center,
        modifier = Modifier.fillMaxWidth().heightIn(min = 24.dp),
    )
}

/**
 * Just enough markdown for agents' replies: **bold**, `code`, headings,
 * bullets and fenced code blocks. Anything else stays as written.
 */
@Composable
private fun markdownLite(text: String): AnnotatedString {
    val t = LocalTerm.current
    return remember(text, t) {
        buildAnnotatedString {
            var inFence = false
            text.lines().forEachIndexed { n, raw ->
                if (n > 0) append("\n")
                if (raw.trimStart().startsWith("```")) {
                    inFence = !inFence
                    return@forEachIndexed
                }
                if (inFence) {
                    withStyle(SpanStyle(color = t.fg, background = t.selection.copy(alpha = 0.3f))) { append(raw) }
                    return@forEachIndexed
                }
                val heading = Regex("^#{1,6}\\s+").find(raw)
                val bullet = Regex("^(\\s*)[-*]\\s+").find(raw)
                when {
                    heading != null -> withStyle(SpanStyle(fontWeight = FontWeight.Bold)) {
                        inline(raw.substring(heading.range.last + 1), t)
                    }
                    bullet != null -> {
                        append("${bullet.groupValues[1]}• ")
                        inline(raw.substring(bullet.range.last + 1), t)
                    }
                    else -> inline(raw, t)
                }
            }
        }
    }
}

/** `code` and **bold** within one line. */
private fun AnnotatedString.Builder.inline(line: String, t: TermTheme) {
    var i = 0
    while (i < line.length) {
        when {
            line.startsWith("**", i) && line.indexOf("**", i + 2) > i + 2 -> {
                val end = line.indexOf("**", i + 2)
                withStyle(SpanStyle(fontWeight = FontWeight.Bold)) { append(line.substring(i + 2, end)) }
                i = end + 2
            }
            line[i] == '`' && line.indexOf('`', i + 1) > i -> {
                val end = line.indexOf('`', i + 1)
                withStyle(SpanStyle(color = t.accent)) { append(line.substring(i + 1, end)) }
                i = end + 1
            }
            else -> {
                append(line[i])
                i++
            }
        }
    }
}
