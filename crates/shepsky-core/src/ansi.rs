//! ANSI screen text -> styled [`Frame`], via the `vt100` terminal emulator.
//!
//! The bridge polls `pane.read {source:"visible", format:"ansi"}`, which
//! returns the visible screen as newline-joined, SGR-styled lines. We feed
//! that through vt100 and emit rows of merged spans, so the phone renders
//! plain styled text and never needs a terminal emulator.
//!
//! Only built with the `ansi` feature (the bridge); the Android cdylib skips
//! it.

use crate::protocol::{Color, Frame, Span};

const MAX_ROWS: u16 = 300;
const COLS: u16 = 400; // generous ceiling; herdr pre-wraps to the pane width

/// Parse one visible-screen read into a frame. Trailing unstyled blanks are
/// trimmed per line, and trailing empty lines are dropped.
pub fn parse_frame(ansi_text: &str) -> Frame {
    // Rows = newline count + 1 (NOT `lines()`, which ignores trailing empty
    // lines — undersizing the parser makes trailing LFs scroll content away).
    let newlines = ansi_text.bytes().filter(|&b| b == b'\n').count();
    let rows = ((newlines + 1).max(1) as u16).min(MAX_ROWS);
    let mut parser = vt100::Parser::new(rows, COLS, 0);
    // herdr joins screen lines with bare `\n`; a terminal needs CRLF to also
    // return the cursor to column 0.
    let normalized = ansi_text.replace('\n', "\r\n");
    parser.process(normalized.as_bytes());
    let screen = parser.screen();

    let mut lines: Vec<Vec<Span>> = Vec::with_capacity(rows as usize);
    let mut max_width: u16 = 0;

    for row in 0..rows {
        let mut spans: Vec<Span> = Vec::new();
        let mut width: u16 = 0; // rightmost column that must be kept
        let mut col: u16 = 0;
        while col < COLS {
            let Some(cell) = screen.cell(row, col) else { break };
            let mut text = cell.contents();
            if text.is_empty() {
                text = " ".to_string();
            }
            let styled = style_of(cell);
            let is_blank = text.trim().is_empty() && styled.bg.is_none() && !styled.r;
            if !is_blank {
                width = col + if cell.is_wide() { 2 } else { 1 };
            }
            match spans.last_mut() {
                Some(last) if same_style(last, &styled) => last.t.push_str(&text),
                _ => {
                    let mut span = styled;
                    span.t = text;
                    spans.push(span);
                }
            }
            col += if cell.is_wide() { 2 } else { 1 };
        }
        // Trim spans beyond the significant width.
        let mut kept: Vec<Span> = Vec::with_capacity(spans.len());
        let mut acc: u16 = 0;
        for mut span in spans {
            if acc >= width {
                break;
            }
            let span_width = span.t.chars().count() as u16; // approximation is fine for trailing-blank trimming
            if acc + span_width > width {
                let keep = (width - acc) as usize;
                span.t = span.t.chars().take(keep).collect();
            }
            acc += span_width;
            kept.push(span);
        }
        max_width = max_width.max(width);
        lines.push(kept);
    }

    // Drop trailing empty lines.
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }

    Frame {
        cols: max_width,
        rows: lines.len() as u16,
        lines,
    }
}

fn style_of(cell: &vt100::Cell) -> Span {
    Span {
        t: String::new(),
        fg: color_of(cell.fgcolor()),
        bg: color_of(cell.bgcolor()),
        b: cell.bold(),
        i: cell.italic(),
        u: cell.underline(),
        r: cell.inverse(),
    }
}

fn same_style(a: &Span, b: &Span) -> bool {
    a.fg == b.fg && a.bg == b.bg && a.b == b.b && a.i == b.i && a.u == b.u && a.r == b.r
}

/// The 16 ANSI colors stay indexes (the phone's theme draws them); the
/// 256-color extensions and truecolor are exact.
fn color_of(color: vt100::Color) -> Option<Color> {
    match color {
        vt100::Color::Default => None,
        vt100::Color::Idx(i @ 0..=15) => Some(Color::Ansi(i)),
        vt100::Color::Idx(i) => Some(Color::Rgb(xterm_256_hex(i))),
        vt100::Color::Rgb(r, g, b) => Some(Color::Rgb(format!("#{r:02x}{g:02x}{b:02x}"))),
    }
}

/// The xterm-256 cube and grayscale ramp (indexes 16–255).
fn xterm_256_hex(i: u8) -> String {
    match i {
        0..=15 => unreachable!("ANSI 0-15 are theme colors"),
        16..=231 => {
            let i = i - 16;
            let steps = [0u8, 95, 135, 175, 215, 255];
            let r = steps[(i / 36) as usize];
            let g = steps[((i % 36) / 6) as usize];
            let b = steps[(i % 6) as usize];
            format!("#{r:02x}{g:02x}{b:02x}")
        }
        232..=255 => {
            let v = 8 + (i - 232) * 10;
            format!("#{v:02x}{v:02x}{v:02x}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_single_span() {
        let frame = parse_frame("hello world");
        assert_eq!(frame.rows, 1);
        assert_eq!(frame.lines[0].len(), 1);
        assert_eq!(frame.lines[0][0].t, "hello world");
        assert_eq!(frame.cols, 11);
    }

    #[test]
    fn colors_and_bold_split_spans() {
        // red "err" then default " ok", bold
        let frame = parse_frame("\x1b[31merr\x1b[0m\x1b[1m ok\x1b[0m");
        let line = &frame.lines[0];
        assert_eq!(line.len(), 2);
        assert_eq!(line[0].t, "err");
        assert_eq!(line[0].fg, Some(Color::Ansi(1)));
        assert_eq!(line[1].t, " ok");
        assert!(line[1].b);
    }

    #[test]
    fn trailing_blanks_trimmed_but_styled_bg_kept() {
        let frame = parse_frame("ab   \nx\x1b[44m   \x1b[0m   ");
        assert_eq!(frame.lines[0].len(), 1);
        assert_eq!(frame.lines[0][0].t, "ab");
        // Second line keeps the blue-background run, drops the unstyled tail.
        let line1: String = frame.lines[1].iter().map(|s| s.t.as_str()).collect();
        assert_eq!(line1, "x   ");
        assert_eq!(frame.lines[1].last().unwrap().bg, Some(Color::Ansi(4)));
    }

    #[test]
    fn multiline_keeps_column_zero() {
        // Without the \n -> \r\n fix, line 2 would start at column 5.
        let frame = parse_frame("first\nsecond");
        assert_eq!(frame.rows, 2);
        assert_eq!(frame.lines[1][0].t, "second");
    }

    #[test]
    fn trailing_empty_lines_dropped() {
        let frame = parse_frame("only\n\n\n");
        assert_eq!(frame.rows, 1);
    }

    #[test]
    fn rgb_and_256_colors() {
        let frame = parse_frame("\x1b[38;2;10;20;30mrgb\x1b[0m\x1b[38;5;196midx\x1b[0m");
        let line = &frame.lines[0];
        assert_eq!(line[0].fg, Some(Color::Rgb("#0a141e".into())));
        assert_eq!(line[1].fg, Some(Color::Rgb("#ff0000".into())));
    }

    #[test]
    fn bright_ansi_stays_an_index() {
        let frame = parse_frame("\x1b[91mhot\x1b[0m");
        assert_eq!(frame.lines[0][0].fg, Some(Color::Ansi(9)));
    }

    #[test]
    fn inverse_is_a_flag_not_a_swap() {
        // Reverse video over default colors: the phone swaps its theme's
        // fg/bg, and a reversed blank still counts (it's visible).
        let frame = parse_frame("a\x1b[7m  \x1b[0m");
        let line = &frame.lines[0];
        assert_eq!(line.len(), 2);
        assert_eq!(line[1].t, "  ");
        assert!(line[1].r);
        assert_eq!(line[1].fg, None);
    }
}
