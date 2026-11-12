//! Turning sample text into glyph runs for the text (spacing) view.
//!
//! Characters map to glyphs by Unicode. `/name` inserts a glyph by name (ended by
//! a space, another `/` or the end of the text); a `/` not followed by a name is
//! the slash itself. Line breaks start a new line.

use std::collections::HashMap;

use serde::Serialize;

use crate::model::Font;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Token {
    Glyph {
        name: String,
    },
    /// A character or `/name` that the font does not contain.
    Missing {
        text: String,
    },
}

fn is_name_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '.' | '_' | '-')
}

/// Splits `text` into lines of glyph tokens.
pub fn parse(font: &Font, text: &str) -> Vec<Vec<Token>> {
    let by_cp: HashMap<u32, &str> = font
        .glyphs
        .iter()
        .flat_map(|g| g.unicodes.iter().map(move |c| (c.0, g.name.as_str())))
        .collect();
    let mut lines = Vec::new();
    for line in text.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        let mut tokens = Vec::new();
        let chars: Vec<char> = line.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            if c == '/' && i + 1 < chars.len() && is_name_char(chars[i + 1]) {
                let start = i + 1;
                let mut end = start;
                while end < chars.len() && is_name_char(chars[end]) {
                    end += 1;
                }
                let name: String = chars[start..end].iter().collect();
                tokens.push(if font.glyph(&name).is_some() {
                    Token::Glyph { name }
                } else {
                    Token::Missing {
                        text: format!("/{name}"),
                    }
                });
                // A single space ends the name and is not drawn.
                i = if end < chars.len() && chars[end] == ' ' {
                    end + 1
                } else {
                    end
                };
                continue;
            }
            tokens.push(match by_cp.get(&(c as u32)) {
                Some(name) => Token::Glyph {
                    name: (*name).to_string(),
                },
                None => Token::Missing {
                    text: c.to_string(),
                },
            });
            i += 1;
        }
        lines.push(tokens);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample::sample_font;

    fn names(line: &[Token]) -> Vec<String> {
        line.iter()
            .map(|t| match t {
                Token::Glyph { name } => name.clone(),
                Token::Missing { text } => format!("?{text}"),
            })
            .collect()
    }

    #[test]
    fn characters_and_names() {
        let f = sample_font();
        let lines = parse(&f, "HO/space/H n/nope é\nTo");
        assert_eq!(lines.len(), 2);
        assert_eq!(
            names(&lines[0]),
            ["H", "O", "space", "H", "n", "?/nope", "?é"]
        );
        assert_eq!(names(&lines[1]), ["T", "o"]);
    }

    #[test]
    fn lone_slash_is_a_slash() {
        let f = sample_font();
        assert_eq!(
            names(&parse(&f, "a / b")[0]),
            ["a", "space", "slash", "space", "b"]
        );
        assert_eq!(names(&parse(&f, "")[0]), Vec::<String>::new());
    }
}
