//! Splash banner for the launcher / proxy mode.
//!
//! Drawn with rounded box-drawing characters so the session info reads
//! cleanly above whatever TUI `claude` paints. Each row is fixed-width to
//! keep the box square; long values get a `…` suffix.

use std::path::Path;

const INNER: usize = 74;

const TL: char = '╭';
const TR: char = '╮';
const BL: char = '╰';
const BR: char = '╯';
const H: char = '─';
const V: char = '│';

pub struct Banner {
    title: String,
    rows: Vec<(String, String)>,
    footer_note: Option<String>,
}

impl Banner {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            rows: Vec::new(),
            footer_note: Some("logs are append-only and kept forever".to_string()),
        }
    }

    pub fn row(mut self, label: impl Into<String>, value: impl Into<String>) -> Self {
        self.rows.push((label.into(), value.into()));
        self
    }

    pub fn render(&self) -> String {
        let mut out = String::new();
        // Top border with embedded title: ╭──── title ────────╮
        let title_segment = format!("{H}{H}{H}{H} {} ", self.title);
        let title_w = title_segment.chars().count();
        let pad = INNER.saturating_sub(title_w);
        out.push(TL);
        out.push_str(&title_segment);
        for _ in 0..pad {
            out.push(H);
        }
        out.push(TR);
        out.push('\n');

        push_blank(&mut out);
        for (k, v) in &self.rows {
            push_row(&mut out, k, v);
        }
        if let Some(note) = &self.footer_note {
            push_blank(&mut out);
            push_row(&mut out, "", note);
        }
        push_blank(&mut out);

        out.push(BL);
        for _ in 0..INNER {
            out.push(H);
        }
        out.push(BR);
        out.push('\n');
        out
    }
}

fn push_blank(s: &mut String) {
    s.push(V);
    for _ in 0..INNER {
        s.push(' ');
    }
    s.push(V);
    s.push('\n');
}

fn push_row(s: &mut String, label: &str, value: &str) {
    const LABEL_COL: usize = 10;
    const LEFT_PAD: usize = 2;
    const GAP: usize = 1;

    let mut line = String::new();
    for _ in 0..LEFT_PAD {
        line.push(' ');
    }
    if !label.is_empty() {
        line.push_str(label);
        let pad = LABEL_COL.saturating_sub(label.chars().count());
        for _ in 0..pad {
            line.push(' ');
        }
        for _ in 0..GAP {
            line.push(' ');
        }
    } else {
        for _ in 0..(LABEL_COL + GAP) {
            line.push(' ');
        }
    }

    let consumed = line.chars().count();
    let avail = INNER.saturating_sub(consumed);
    let v = truncate(value, avail);
    line.push_str(&v);

    let cur = line.chars().count();
    let pad = INNER.saturating_sub(cur);
    for _ in 0..pad {
        line.push(' ');
    }

    s.push(V);
    s.push_str(&line);
    s.push(V);
    s.push('\n');
}

fn truncate(text: &str, max: usize) -> String {
    let cw = text.chars().count();
    if cw <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

pub fn abbrev_path(p: &Path) -> String {
    if let Some(home) = dirs::home_dir() {
        if let Ok(rest) = p.strip_prefix(&home) {
            return format!("~/{}", rest.display());
        }
    }
    p.display().to_string()
}
