//! Valve's text KeyValues format, which Steam keeps its library in
//! (`libraryfolders.vdf`, `appmanifest_<appid>.acf`):
//!
//! ```text
//! "AppState"
//! {
//!     "appid"       "228980"
//!     "installdir"  "Steamworks Shared"
//! }
//! ```
//!
//! Keys and values are quoted (or bare words), a value is text or a `{ }` block of
//! more pairs, and `//` starts a comment. Conditionals like `[$WIN32]` aren't
//! supported; Steam doesn't write them in these files.

/// A value: text, or a block of key-value pairs.
#[derive(Debug, PartialEq)]
pub enum Value {
    Text(String),
    Block(Block),
}

/// Key-value pairs in file order. A key may repeat; `get` finds the first.
#[derive(Debug, PartialEq, Default)]
pub struct Block(Vec<(String, Value)>);

impl Block {
    /// The first value under `key`, in any case, the way Steam reads it.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.0.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v)
    }

    pub fn text(&self, key: &str) -> Option<&str> {
        match self.get(key)? {
            Value::Text(t) => Some(t),
            Value::Block(_) => None,
        }
    }

    pub fn block(&self, key: &str) -> Option<&Block> {
        match self.get(key)? {
            Value::Block(b) => Some(b),
            Value::Text(_) => None,
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &Value)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v))
    }
}

#[derive(Debug, PartialEq)]
enum Token {
    Text(String),
    Open,
    Close,
}

/// Parses a whole file. The result is the top level, usually a single block like
/// `"AppState" { ... }`.
pub fn parse(text: &str) -> Result<Block, String> {
    let mut tokens = Tokens { rest: text, line: 1 };
    let block = parse_block(&mut tokens)?;
    match tokens.next()? {
        None => Ok(block),
        Some(_) => Err(format!("line {}: a }} without a {{", tokens.line)),
    }
}

/// Pairs up to the `}` that closes the block, or the end of the text at the top level.
fn parse_block(tokens: &mut Tokens) -> Result<Block, String> {
    let mut pairs = Vec::new();
    loop {
        let key = match tokens.peek()? {
            Some(Token::Text(_)) => match tokens.next()? {
                Some(Token::Text(k)) => k,
                _ => unreachable!("peeked a text token"),
            },
            // the caller deals with it: the end of the block, or one too many at the top
            Some(Token::Close) | None => return Ok(Block(pairs)),
            Some(Token::Open) => return Err(format!("line {}: a {{ without a key", tokens.line)),
        };
        let value = match tokens.next()? {
            Some(Token::Text(v)) => Value::Text(v),
            Some(Token::Open) => {
                let inner = parse_block(tokens)?;
                if tokens.next()? != Some(Token::Close) {
                    return Err(format!("the block under {key:?} isn't closed"));
                }
                Value::Block(inner)
            }
            Some(Token::Close) | None => return Err(format!("line {}: {key:?} has no value", tokens.line)),
        };
        pairs.push((key, value));
    }
}

struct Tokens<'a> {
    rest: &'a str,
    /// Where `rest` starts, for errors.
    line: usize,
}

impl Tokens<'_> {
    fn peek(&self) -> Result<Option<Token>, String> {
        Tokens { rest: self.rest, line: self.line }.next()
    }

    fn next(&mut self) -> Result<Option<Token>, String> {
        loop {
            let trimmed = self.rest.trim_start();
            self.line += self.rest[..self.rest.len() - trimmed.len()].matches('\n').count();
            self.rest = trimmed;
            match self.rest.strip_prefix("//") {
                Some(comment) => self.rest = comment.find('\n').map_or("", |end| &comment[end..]),
                None => break,
            }
        }
        let mut chars = self.rest.chars();
        let token = match chars.next() {
            None => return Ok(None),
            Some('{') => Token::Open,
            Some('}') => Token::Close,
            Some('"') => {
                let mut text = String::new();
                loop {
                    match chars.next() {
                        None => return Err(format!("line {}: a quote isn't closed", self.line)),
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some('n') => text.push('\n'),
                            Some('t') => text.push('\t'),
                            // \\ and \", and anything else as it is
                            Some(c) => text.push(c),
                            None => return Err(format!("line {}: a quote isn't closed", self.line)),
                        },
                        Some(c) => {
                            if c == '\n' {
                                self.line += 1;
                            }
                            text.push(c);
                        }
                    }
                }
                Token::Text(text)
            }
            Some(_) => {
                let end = self.rest.find(|c: char| c.is_whitespace() || "{}\"".contains(c)).unwrap_or(self.rest.len());
                let word = self.rest[..end].to_string();
                self.rest = &self.rest[end..];
                return Ok(Some(Token::Text(word)));
            }
        };
        self.rest = chars.as_str();
        Ok(Some(token))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_nested_blocks_escapes_comments_and_bare_words() {
        let text = r#"
// written by Steam
"libraryfolders"
{
    "0"
    {
        "path"      "C:\\Program Files (x86)\\Steam"
        "apps" { "228980" "1234" }
    }
    "1"   "/mnt/games"
    Name  "Say \"hi\""
}
"#;
        let top = parse(text).unwrap();
        let folders = top.block("LibraryFolders").expect("keys are matched in any case");
        assert_eq!(folders.block("0").unwrap().text("path"), Some(r"C:\Program Files (x86)\Steam"));
        assert_eq!(folders.block("0").unwrap().block("apps").unwrap().text("228980"), Some("1234"));
        assert_eq!(folders.text("1"), Some("/mnt/games"));
        assert_eq!(folders.text("name"), Some("Say \"hi\""));
        assert_eq!(folders.text("0"), None, "a block isn't text");
        let keys: Vec<_> = folders.iter().map(|(k, _)| k).collect();
        assert_eq!(keys, ["0", "1", "Name"]);
    }

    #[test]
    fn broken_files_are_errors_with_a_line() {
        assert!(parse("\"a\"\n{\n\"b\" \"c\"\n").unwrap_err().contains("isn't closed"));
        assert_eq!(parse("\"a\" \"b\"\n\n}").unwrap_err(), "line 3: a } without a {");
        assert_eq!(parse("\"a\" { \"b\" }").unwrap_err(), "line 1: \"b\" has no value");
        assert_eq!(parse("\n\n\"a\" \"unclosed").unwrap_err(), "line 3: a quote isn't closed");
        assert_eq!(parse("{ }").unwrap_err(), "line 1: a { without a key");
        assert_eq!(parse("").unwrap(), Block::default());
    }
}
