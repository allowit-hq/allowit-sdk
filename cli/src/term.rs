//! Terminal output. Data goes to stdout; progress, links, hints and prompts go
//! to stderr. Text is the default, also when piped; `--json` (or
//! `--output json`) selects JSON. Policy source and JSON documents are
//! syntax-highlighted only when stdout is a terminal, and never with `--json`,
//! `NO_COLOR`, `TERM=dumb` or `--no-color`. `CLICOLOR_FORCE` forces color.
use serde_json::Value;
use std::io::IsTerminal;

/// What stdout holds, for highlighting at flush time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Text,
    Json,
    Rust,
}

/// Buffered command output, written once when the command ends.
pub(crate) struct Out {
    pub stdout: String,
    pub stderr: String,
    pub json: bool,
    /// `NO_DNA`: compact JSON, no prompts.
    pub compact: bool,
    pub quiet: bool,
    pub verbose: bool,
    pub color_out: bool,
    pub color_err: bool,
    pub kind: Kind,
    /// Print stdout unchanged: no redaction and no display escaping. The local
    /// policy compiler commands use it.
    pub raw: bool,
    /// The request this command acts on, for JSON errors.
    pub request_id: Option<String>,
}

fn env_set(name: &str) -> bool {
    std::env::var_os(name).is_some_and(|v| !v.is_empty())
}

/// True when color may be written to a stream that is a terminal.
pub(crate) fn color_allowed(no_color_flag: bool, terminal: bool) -> bool {
    if no_color_flag || env_set("NO_COLOR") || std::env::var("TERM").is_ok_and(|t| t == "dumb") {
        return false;
    }
    env_set("CLICOLOR_FORCE") || terminal
}

impl Out {
    pub(crate) fn new(json: bool, quiet: bool, verbose: bool, no_color: bool) -> Self {
        let compact = env_set("NO_DNA");
        Self {
            stdout: String::new(),
            stderr: String::new(),
            json,
            compact,
            quiet,
            verbose,
            color_out: !json
                && !compact
                && color_allowed(no_color, std::io::stdout().is_terminal()),
            color_err: !compact && color_allowed(no_color, std::io::stderr().is_terminal()),
            kind: Kind::Text,
            raw: false,
            request_id: None,
        }
    }
    /// One line of text data.
    pub(crate) fn line(&mut self, text: impl AsRef<str>) {
        self.stdout.push_str(text.as_ref());
        self.stdout.push('\n');
    }
    /// A structured result. JSON mode prints JSON; text mode prints `key: value`
    /// lines for a flat object and an indented JSON document otherwise.
    pub(crate) fn value(&mut self, value: &Value) {
        if self.json {
            self.json_value(value);
            return;
        }
        match value {
            Value::Object(map) if map.values().all(|v| !v.is_object() && !v.is_array()) => {
                let width = map.keys().map(String::len).max().unwrap_or(0);
                for (k, v) in map {
                    let text = match v {
                        Value::String(s) => s.clone(),
                        Value::Null => "-".into(),
                        other => other.to_string(),
                    };
                    self.line(format!("{k:width$}  {text}"));
                }
            }
            Value::String(s) => self.line(s),
            other => {
                self.json_value(other);
            }
        }
    }
    /// A JSON document, highlighted on a terminal in text mode.
    pub(crate) fn json_value(&mut self, value: &Value) {
        let text = if self.compact {
            serde_json::to_string(value)
        } else {
            serde_json::to_string_pretty(value)
        }
        .unwrap_or_default();
        self.stdout.push_str(&text);
        self.stdout.push('\n');
        if self.kind == Kind::Text {
            self.kind = Kind::Json;
        }
    }
    /// Rust policy source, highlighted on a terminal.
    pub(crate) fn source(&mut self, rust: &str) {
        self.stdout.push_str(rust);
        if !rust.ends_with('\n') {
            self.stdout.push('\n');
        }
        self.kind = Kind::Rust;
    }
    /// A table of rows; the first row is the header. Text mode only.
    pub(crate) fn table(&mut self, rows: &[Vec<String>]) {
        let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
        let widths: Vec<usize> = (0..columns)
            .map(|c| {
                rows.iter()
                    .filter_map(|r| r.get(c))
                    .map(|s| s.chars().count())
                    .max()
                    .unwrap_or(0)
            })
            .collect();
        for row in rows {
            let mut line = String::new();
            for (c, cell) in row.iter().enumerate() {
                if c + 1 == row.len() {
                    line.push_str(cell);
                } else {
                    line.push_str(&format!("{cell:width$}  ", width = widths[c]));
                }
            }
            self.line(line.trim_end());
        }
    }
    /// Progress or a link on stderr. `-q` drops it.
    pub(crate) fn progress(&mut self, text: impl AsRef<str>) {
        if !self.quiet {
            self.stderr.push_str(text.as_ref());
            self.stderr.push('\n');
        }
    }
    /// Extra diagnostics on stderr with `-v`.
    pub(crate) fn detail(&mut self, text: impl AsRef<str>) {
        if self.verbose {
            self.stderr.push_str(text.as_ref());
            self.stderr.push('\n');
        }
    }
    /// The final stdout text, highlighted when allowed.
    pub(crate) fn rendered_stdout(&self, stdout: String) -> String {
        if !self.color_out || stdout.is_empty() {
            return stdout;
        }
        match self.kind {
            Kind::Rust => highlight(&stdout, "rs"),
            Kind::Json => highlight(&stdout, "json"),
            Kind::Text if serde_json::from_str::<Value>(&stdout).is_ok() => {
                highlight(&stdout, "json")
            }
            Kind::Text => stdout,
        }
    }
    /// An error label for stderr, red on a terminal.
    pub(crate) fn error_label(&self) -> &'static str {
        if self.color_err {
            "\x1b[1;31merror:\x1b[0m"
        } else {
            "error:"
        }
    }
}

/// Highlights text with the syntax for a file extension, as 24-bit ANSI color.
pub(crate) fn highlight(text: &str, extension: &str) -> String {
    use syntect::{
        easy::HighlightLines,
        highlighting::ThemeSet,
        parsing::SyntaxSet,
        util::{LinesWithEndings, as_24_bit_terminal_escaped},
    };
    let syntaxes = SyntaxSet::load_defaults_newlines();
    let Some(syntax) = syntaxes.find_syntax_by_extension(extension) else {
        return text.to_owned();
    };
    let themes = ThemeSet::load_defaults();
    let Some(theme) = themes.themes.get("base16-ocean.dark") else {
        return text.to_owned();
    };
    let mut lines = HighlightLines::new(syntax, theme);
    let mut out = String::with_capacity(text.len() * 2);
    for line in LinesWithEndings::from(text) {
        match lines.highlight_line(line, &syntaxes) {
            Ok(ranges) => out.push_str(&as_24_bit_terminal_escaped(&ranges, false)),
            Err(_) => return text.to_owned(),
        }
    }
    out.push_str("\x1b[0m");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn plain(json: bool) -> Out {
        let mut out = Out::new(json, false, false, true);
        out.compact = false;
        out
    }
    #[test]
    fn flat_objects_print_as_aligned_lines_in_text_mode() {
        let mut out = plain(false);
        out.value(&json!({"name":"agent","address":"GABC"}));
        // Key order follows serde_json's map order; alignment does not.
        let mut lines: Vec<&str> = out.stdout.lines().collect();
        lines.sort();
        assert_eq!(lines, ["address  GABC", "name     agent"]);
        let mut out = plain(true);
        out.value(&json!({"name":"agent"}));
        assert_eq!(out.stdout, "{\n  \"name\": \"agent\"\n}\n");
    }
    #[test]
    fn highlighting_adds_color_and_keeps_the_text() {
        let source = "fn main() { let x = 1; }\n";
        let colored = highlight(source, "rs");
        assert!(colored.contains("\x1b["));
        let stripped = regex::Regex::new("\x1b\\[[0-9;]*m")
            .unwrap()
            .replace_all(&colored, "");
        assert_eq!(stripped, source);
        let mut out = plain(false);
        out.source(source);
        assert_eq!(out.rendered_stdout(out.stdout.clone()), source);
        out.color_out = true;
        assert!(out.rendered_stdout(out.stdout.clone()).contains("\x1b["));
    }
    #[test]
    fn quiet_drops_progress_but_not_data() {
        let mut out = Out::new(false, true, false, true);
        out.progress("working");
        out.line("data");
        assert!(out.stderr.is_empty());
        assert_eq!(out.stdout, "data\n");
    }
}
