//! Incremental bounded OSC notification decoding before native desktop truncation/throttling.
use crate::inbox::Content;
use base64::Engine;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use uuid::Uuid;

const FRAME_LIMIT: usize = 16384;
const PARTIAL_LIMIT: usize = 8;
static OUTPUT_BYTES: AtomicU64 = AtomicU64::new(0);
static PARSE_NS: AtomicU64 = AtomicU64::new(0);
static ACCEPTED: AtomicU64 = AtomicU64::new(0);
static REJECTED: AtomicU64 = AtomicU64::new(0);
static OVERSIZE_FRAMES: AtomicU64 = AtomicU64::new(0);

/// Read content-free parser workload and admission counters for end-to-end diagnostics.
pub fn metrics() -> serde_json::Value {
    serde_json::json!({"output_bytes":OUTPUT_BYTES.load(Ordering::Relaxed), "parse_ns":PARSE_NS.load(Ordering::Relaxed),
        "accepted":ACCEPTED.load(Ordering::Relaxed), "queue_rejected":REJECTED.load(Ordering::Relaxed), "oversize_frames":OVERSIZE_FRAMES.load(Ordering::Relaxed)})
}

/// Surface-owned callback context; Ghostty stops its IO thread before the owning allocation is freed.
pub struct Context {
    surface: Uuid,
    parser: Mutex<Parser>,
}

impl Context {
    /// Allocate independent stream and chunk state for an exact terminal tab, before native startup.
    pub fn new(surface: Uuid) -> Self {
        Self {
            surface,
            parser: Mutex::new(Parser::default()),
        }
    }
}

/// Observe local or manual remote output without retaining raw terminal bytes or accessing GTK.
/// # Safety
/// Userdata must point to a live Context and bytes must be readable for length bytes. The host retains
/// the context until ghostty_surface_free has stopped every caller of this callback.
pub unsafe extern "C" fn output(
    userdata: *mut std::ffi::c_void,
    bytes: *const std::ffi::c_char,
    length: usize,
) {
    if userdata.is_null() || bytes.is_null() || length == 0 {
        return;
    }
    // SAFETY: the native callback contract supplies these live allocations for this invocation.
    let (context, bytes) = unsafe {
        (
            &*userdata.cast::<Context>(),
            std::slice::from_raw_parts(bytes.cast::<u8>(), length),
        )
    };
    if let Ok(mut parser) = context.parser.lock() {
        let started = Instant::now();
        OUTPUT_BYTES.fetch_add(length as u64, Ordering::Relaxed);
        parser.feed(bytes, |content| {
            let accepted = super::events::push(super::events::Event::Notification {
                surface: context.surface,
                content,
            });
            if accepted { &ACCEPTED } else { &REJECTED }.fetch_add(1, Ordering::Relaxed);
        });
        PARSE_NS.fetch_add(started.elapsed().as_nanos() as u64, Ordering::Relaxed);
    }
}

#[derive(Default)]
enum State {
    #[default]
    Ground,
    Escape,
    Osc,
    OscEscape,
    Ignore,
    IgnoreEscape,
}

struct Partial {
    content: Content,
    invalid: bool,
    updated: Instant,
}

/// Stream framing retains one bounded OSC and at most eight bounded incomplete OSC99 messages.
#[derive(Default)]
struct Parser {
    state: State,
    frame: Vec<u8>,
    overflow: bool,
    partials: HashMap<String, Partial>,
}

impl Parser {
    /// Consume arbitrary read boundaries, ignoring unrelated control strings and oversize frames.
    fn feed(&mut self, bytes: &[u8], mut emit: impl FnMut(Content)) {
        // Ordinary bulk terminal output needs only the optimized byte search, not a bytewise parser.
        if matches!(self.state, State::Ground) && !bytes.contains(&0x1b) {
            return;
        }
        for &byte in bytes {
            if matches!(byte, 0x18 | 0x1a) {
                self.state = State::Ground;
                self.frame.clear();
                continue;
            }
            match self.state {
                State::Ground => {
                    if byte == 0x1b {
                        self.state = State::Escape;
                    }
                }
                State::Escape => {
                    self.state = match byte {
                        b']' => {
                            self.frame.clear();
                            self.overflow = false;
                            State::Osc
                        }
                        b'P' | b'_' | b'^' | b'X' => State::Ignore,
                        0x1b => State::Escape,
                        _ => State::Ground,
                    };
                }
                State::Ignore => {
                    if byte == 0x1b {
                        self.state = State::IgnoreEscape;
                    }
                }
                State::IgnoreEscape => {
                    self.state = if byte == b'\\' {
                        State::Ground
                    } else if byte == 0x1b {
                        State::IgnoreEscape
                    } else {
                        State::Ignore
                    };
                }
                State::Osc => match byte {
                    7 => self.finish(&mut emit),
                    0x1b => self.state = State::OscEscape,
                    _ => {
                        if self.frame.len() < FRAME_LIMIT {
                            self.frame.push(byte);
                        } else {
                            self.overflow = true;
                        }
                    }
                },
                State::OscEscape => {
                    if byte == b'\\' {
                        self.finish(&mut emit);
                    } else {
                        self.frame.clear();
                        self.state = if byte == b']' {
                            self.overflow = false;
                            State::Osc
                        } else {
                            State::Ground
                        };
                    }
                }
            }
        }
    }

    /// Decode one complete frame, returning its reusable allocation without retaining terminal text.
    fn finish(&mut self, emit: &mut impl FnMut(Content)) {
        self.state = State::Ground;
        let mut frame = std::mem::take(&mut self.frame);
        if self.overflow || std::str::from_utf8(&frame).is_err() {
            if self.overflow {
                OVERSIZE_FRAMES.fetch_add(1, Ordering::Relaxed);
            }
            // A discarded chunk must not allow a later completion to publish partial message text.
            for partial in self.partials.values_mut() {
                partial.invalid = true;
            }
        } else {
            if let Ok(text) = std::str::from_utf8(&frame) {
                if let Some(content) = self.decode(text) {
                    emit(content);
                }
            }
        }
        frame.clear();
        self.frame = frame;
    }

    /// Decode supported notification families; ConEmu operations remain the native parser's concern.
    fn decode(&mut self, frame: &str) -> Option<Content> {
        let (code, payload) = frame.split_once(';')?;
        let content = match code {
            "9" => {
                let first = payload.split(';').next()?;
                if (payload.contains(';')
                    && first
                        .parse::<u8>()
                        .is_ok_and(|code| (1..=12).contains(&code)))
                    || matches!(payload, "10" | "11" | "12")
                {
                    return None;
                }
                Content {
                    title: "Notification".into(),
                    subtitle: String::new(),
                    body: payload.into(),
                }
            }
            "777" => {
                let (kind, payload) = payload.split_once(';')?;
                if kind != "notify" {
                    return None;
                }
                let (title, body) = payload.split_once(';').unwrap_or((payload, ""));
                Content {
                    title: title.into(),
                    subtitle: String::new(),
                    body: body.into(),
                }
            }
            "99" => return self.chunk(payload),
            _ => return None,
        };
        content.validate().ok()?;
        Some(content)
    }

    /// Assemble title/body chunks per native ID, with bounded lifetime, count and completed content.
    fn chunk(&mut self, payload: &str) -> Option<Content> {
        let (metadata, payload) = payload.split_once(';')?;
        let (mut id, mut part, mut done, mut encoded) = ("", "title", true, false);
        for item in metadata.split(':').filter(|item| !item.is_empty()) {
            let (key, value) = item.split_once('=')?;
            match key {
                "i" => id = value,
                "p" => part = value,
                "d" => {
                    done = match value {
                        "0" => false,
                        "1" => true,
                        _ => return None,
                    }
                }
                "e" => {
                    encoded = match value {
                        "0" => false,
                        "1" => true,
                        _ => return None,
                    }
                }
                _ => {}
            }
        }
        if id.len() > 128
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        {
            return None;
        }
        if part == "close" {
            self.partials.remove(id);
            return None;
        }
        if !matches!(part, "title" | "body") {
            return None;
        }
        self.partials
            .retain(|_, partial| partial.updated.elapsed() < Duration::from_secs(60));
        if !self.partials.contains_key(id) && self.partials.len() >= PARTIAL_LIMIT {
            let oldest = self
                .partials
                .iter()
                .min_by_key(|(_, partial)| partial.updated)
                .map(|(id, _)| id.clone())?;
            self.partials.remove(&oldest);
        }
        let partial = self.partials.entry(id.into()).or_insert_with(|| Partial {
            content: Content::default(),
            invalid: false,
            updated: Instant::now(),
        });
        partial.updated = Instant::now();
        let decoded = if encoded {
            base64::engine::general_purpose::STANDARD
                .decode(payload)
                .ok()
                .and_then(|bytes| String::from_utf8(bytes).ok())
        } else {
            Some(payload.into())
        };
        if let Some(decoded) = decoded {
            let field = if part == "body" {
                &mut partial.content.body
            } else {
                &mut partial.content.title
            };
            let limit = if part == "body" { 8192 } else { 512 };
            if field.len() + decoded.len() > limit || decoded.contains('\0') {
                partial.invalid = true;
            } else if !partial.invalid {
                field.push_str(&decoded);
            }
        } else {
            partial.invalid = true;
        }
        if !done {
            return None;
        }
        let mut partial = self.partials.remove(id)?;
        if partial.invalid || (partial.content.title.is_empty() && partial.content.body.is_empty())
        {
            return None;
        }
        if partial.content.title.is_empty() {
            partial.content.title = "Notification".into();
        }
        Some(partial.content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Split every byte boundary, assemble interleaved chunks and recover after oversized control data.
    #[test]
    fn framing_chunking_and_limits() {
        let mut parser = Parser::default();
        let mut messages = Vec::new();
        for byte in b"\x1b]99;i=a:d=0;Title\x1b\\\x1b]9;hello\x07\x1b]99;i=a:p=body:e=1;Ym9keQ==\x1b\\\x1b]777;notify;Other;message\x07" {
            parser.feed(&[*byte], |content| messages.push(content));
        }
        assert_eq!(messages.len(), 3);
        assert_eq!(
            (&messages[1].title, &messages[1].body),
            (&"Title".into(), &"body".into())
        );
        parser.feed(
            b"\x1b]9;4;1;50\x07\x1bP\x1b]9;hidden\x07\x1b\\",
            |content| messages.push(content),
        );
        assert_eq!(messages.len(), 3);
        parser.feed(
            format!("\x1b]9;{}\x07\x1b]9;after\x07", "x".repeat(FRAME_LIMIT * 2)).as_bytes(),
            |content| messages.push(content),
        );
        assert_eq!(messages.last().unwrap().body, "after");
        assert!(parser.frame.capacity() <= FRAME_LIMIT * 2);
        for id in 0..100 {
            parser.feed(format!("\x1b]99;i={id}:d=0;pending\x07").as_bytes(), |_| {
                panic!("incomplete notification emitted")
            });
        }
        assert_eq!(parser.partials.len(), PARTIAL_LIMIT);
    }
}
