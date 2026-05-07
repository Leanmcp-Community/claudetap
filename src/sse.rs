//! Minimal SSE (Server-Sent Events) parser.
//!
//! Just enough to split a `text/event-stream` response into individual events
//! so we can write them one-per-line to the session log. This is byte-fed
//! and stateful; feed it whatever chunks arrive off the wire and it returns
//! whole events as they complete.

use time::OffsetDateTime;

use crate::log::SseEvent;

#[derive(Default)]
pub struct SseParser {
    buf: Vec<u8>,
}

impl SseParser {
    pub fn new() -> Self {
        Self { buf: Vec::new() }
    }

    pub fn feed(&mut self, data: &[u8]) -> Vec<SseEvent> {
        self.buf.extend_from_slice(data);
        let mut out = Vec::new();
        loop {
            let Some(end) = find_event_end(&self.buf) else {
                break;
            };
            let block: Vec<u8> = self.buf.drain(..end).collect();
            let s = String::from_utf8_lossy(&block);
            if let Some(ev) = parse_event(&s) {
                out.push(ev);
            }
        }
        out
    }

    /// Flush whatever is left in the buffer as a final partial event.
    /// Useful when the upstream connection closes mid-event.
    pub fn flush_remaining(&mut self) -> Option<SseEvent> {
        if self.buf.is_empty() {
            return None;
        }
        let s = String::from_utf8_lossy(&self.buf);
        let ev = parse_event(&s);
        self.buf.clear();
        ev
    }
}

fn find_event_end(buf: &[u8]) -> Option<usize> {
    let mut i = 0;
    while i < buf.len() {
        if buf[i] == b'\n' && i + 1 < buf.len() && buf[i + 1] == b'\n' {
            return Some(i + 2);
        }
        if i + 3 < buf.len() && &buf[i..i + 4] == b"\r\n\r\n" {
            return Some(i + 4);
        }
        if buf[i] == b'\r' && i + 1 < buf.len() && buf[i + 1] == b'\r' {
            return Some(i + 2);
        }
        i += 1;
    }
    None
}

fn parse_event(block: &str) -> Option<SseEvent> {
    let mut event: Option<String> = None;
    let mut id: Option<String> = None;
    let mut retry: Option<u64> = None;
    let mut data_lines: Vec<&str> = Vec::new();
    let mut had_field = false;

    for line in block.split(|c| c == '\n' || c == '\r') {
        if line.is_empty() {
            continue;
        }
        if let Some(stripped) = line.strip_prefix(':') {
            // comment
            let _ = stripped;
            continue;
        }
        had_field = true;
        let (field, value) = match line.find(':') {
            Some(idx) => {
                let v = &line[idx + 1..];
                let v = v.strip_prefix(' ').unwrap_or(v);
                (&line[..idx], v)
            }
            None => (line, ""),
        };
        match field {
            "event" => event = Some(value.to_string()),
            "id" => id = Some(value.to_string()),
            "retry" => retry = value.parse::<u64>().ok(),
            "data" => data_lines.push(value),
            _ => {}
        }
    }

    if !had_field {
        return None;
    }

    Some(SseEvent {
        ts: OffsetDateTime::now_utc(),
        event,
        id,
        retry,
        data: data_lines.join("\n"),
    })
}
