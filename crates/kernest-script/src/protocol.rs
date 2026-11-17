//! The JSON contract between Kernest and the bundled Python process.
//!
//! Kernest writes one [`Request`] line to the child's stdin and closes it. The
//! child writes newline-delimited [`Message`]s to stdout: streamed output while
//! it runs, then exactly one result. Keeping the protocol separate from the
//! runner means both sides can be tested without spawning a process.

use kernest_core::Font;
use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 1;

/// What the child is being asked to do. Scripts and macros may change the font;
/// filters run on the selected glyphs; reporters only return drawing instructions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Script,
    Macro,
    Filter,
    Reporter,
}

/// The editor selection handed to the script so it knows what to act on.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Selection {
    #[serde(default)]
    pub glyphs: Vec<String>,
    pub current_glyph: Option<String>,
    pub master_id: String,
    #[serde(default)]
    pub layers: Vec<String>,
}

/// One request to the Python process. `code` carries an inline macro; `path`
/// carries a script or plug-in entry file. Exactly one of them is set.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub protocol: u32,
    pub kind: Kind,
    pub font: Font,
    pub selection: Selection,
    #[serde(default)]
    pub params: serde_json::Value,
    pub code: Option<String>,
    pub path: Option<String>,
}

impl Request {
    pub fn new(kind: Kind, font: Font, selection: Selection) -> Self {
        Self {
            protocol: PROTOCOL_VERSION,
            kind,
            font,
            selection,
            params: serde_json::Value::Null,
            code: None,
            path: None,
        }
    }
}

/// A drawing instruction a reporter returns, in font units over the canvas.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum DrawItem {
    Line {
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
        #[serde(default)]
        color: Option<String>,
        #[serde(default)]
        width: Option<f64>,
    },
    Path {
        d: String,
        #[serde(default)]
        color: Option<String>,
        #[serde(default)]
        fill: Option<String>,
    },
    Text {
        x: f64,
        y: f64,
        text: String,
        #[serde(default)]
        size: Option<f64>,
        #[serde(default)]
        color: Option<String>,
    },
}

/// A line of streamed output from the script's `print` or stderr.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputLine {
    pub stream: String,
    pub text: String,
}

/// A message read from the child's stdout.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Message {
    Output {
        stream: String,
        text: String,
    },
    Result {
        ok: bool,
        #[serde(default)]
        font: Option<Box<Font>>,
        #[serde(default)]
        draw: Vec<DrawItem>,
        #[serde(default)]
        error: Option<String>,
    },
}

/// Everything a finished run produced: the streamed output, the changed font (if
/// any), reporter drawing instructions, and an error message if it failed.
#[derive(Debug, Clone, Default)]
pub struct RunOutcome {
    pub output: Vec<OutputLine>,
    pub font: Option<Font>,
    pub draw: Vec<DrawItem>,
    pub error: Option<String>,
}

impl RunOutcome {
    pub fn ok(&self) -> bool {
        self.error.is_none()
    }
}

/// Parses one stdout line into a message. Blank lines are ignored by the caller.
pub fn parse_message(line: &str) -> Result<Message, String> {
    serde_json::from_str(line).map_err(|e| format!("Bad script message: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use kernest_core::sample::sample_font;

    #[test]
    fn request_round_trips_through_json() {
        let font = sample_font();
        let mut req = Request::new(Kind::Macro, font.clone(), Selection::default());
        req.code = Some("font.glyphs['A'].width = 500".into());
        let json = serde_json::to_string(&req).unwrap();
        let back: Request = serde_json::from_str(&json).unwrap();
        assert_eq!(back.protocol, PROTOCOL_VERSION);
        assert_eq!(back.kind, Kind::Macro);
        assert_eq!(back.font, font);
        assert_eq!(back.code.as_deref(), Some("font.glyphs['A'].width = 500"));
    }

    #[test]
    fn output_message_parses() {
        let m = parse_message(r#"{"type":"output","stream":"stdout","text":"hi"}"#).unwrap();
        match m {
            Message::Output { stream, text } => {
                assert_eq!(stream, "stdout");
                assert_eq!(text, "hi");
            }
            _ => panic!("expected output"),
        }
    }

    #[test]
    fn result_message_carries_font_and_draw() {
        let original = sample_font();
        let msg = Message::Result {
            ok: true,
            font: Some(Box::new(original.clone())),
            draw: vec![DrawItem::Line {
                x0: 0.0,
                y0: 0.0,
                x1: 100.0,
                y1: 700.0,
                color: None,
                width: None,
            }],
            error: None,
        };
        let line = serde_json::to_string(&msg).unwrap();
        match parse_message(&line).unwrap() {
            Message::Result { ok, font, draw, .. } => {
                assert!(ok);
                assert_eq!(*font.unwrap(), original);
                assert_eq!(draw.len(), 1);
            }
            _ => panic!("expected result"),
        }
    }

    #[test]
    fn bad_message_is_refused() {
        assert!(parse_message("not json").is_err());
        assert!(parse_message(r#"{"type":"unknown"}"#).is_err());
    }
}
