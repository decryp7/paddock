//! A small shell lexer for Bash tool calls (ADR 0083 §2, plan RT-4).
//!
//! It turns a command line into [`Item`]s: simple commands (words with quotes removed, redirections dropped, heredoc bodies
//! kept apart) and the separators between them. Nested shell text is parsed too and appended after the command that holds it:
//! `$(...)` and backticks, `bash -c '...'`, `eval ...`, and a heredoc or here-string fed to a shell. Quoted text and heredoc
//! bodies fed to anything else are data, never commands, so a commit message that mentions `git push -f` does not match.
//!
//! It is not a shell. It never expands variables or globs and never fails on odd input beyond [`LexError`]; the rules fail open
//! on an error (ADR 0083 D7). Bounded: input size, nesting depth and the number of nested parses have limits.
//!
//! brush-parser 0.4.0 was measured first and not taken: it pulls 94 crates (`bon`, `darling`, `cached`, a second `syn` major) for a
//! walk over an AST of which the rules need the flat command list. The numbers are in `docs/benchmarks.md`.

/// The longest command text parsed (the whole input and every nested piece count against it).
pub const MAX_SOURCE: usize = 1 << 20;
/// The deepest nesting of `$(...)`, `bash -c` and `eval`.
const MAX_DEPTH: usize = 6;
/// The most nested pieces parsed for one command.
const MAX_NESTED: usize = 64;

/// Why a command could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LexError {
    /// A quote, `$(`, backtick or `${` was never closed.
    Unterminated(&'static str),
    /// The text or its nesting is past a limit.
    TooLarge,
}

impl std::fmt::Display for LexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LexError::Unterminated(what) => write!(f, "unterminated {what}"),
            LexError::TooLarge => write!(f, "command text or nesting past the limit"),
        }
    }
}

/// One word of a command with its quotes removed. Nothing is expanded.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Word {
    /// The text after quote removal (command substitutions stay out of it).
    pub text: String,
    /// The source of every `$(...)`, backtick and `<(...)` inside the word.
    pub subs: Vec<String>,
    /// The word is `NAME=value` with the name unquoted.
    pub assignment: bool,
}

/// What separates two commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sep {
    /// `;`
    Semi,
    /// `&&`
    And,
    /// `||`
    Or,
    /// `|` and `|&`
    Pipe,
    /// `&`
    Amp,
    /// A line end.
    Newline,
    /// `(`
    Open,
    /// `)`
    Close,
}

/// A simple command: its words (redirections removed) and the text a heredoc or here-string gives its input.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Simple {
    /// The words in order.
    pub words: Vec<Word>,
    /// Heredoc bodies and here-strings.
    pub docs: Vec<String>,
    /// Substitutions in redirection targets (`> $(...)`), which run too.
    pub subs: Vec<String>,
}

/// One element of a parsed command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    /// A simple command.
    Cmd(Simple),
    /// A separator.
    Sep(Sep),
    /// The start of a nested piece (`$(...)`, `bash -c`...): it runs inside the command before it, so it starts with that state
    /// (a loop condition) and the state is restored at the matching [`Item::Leave`].
    Enter,
    /// The end of a nested piece.
    Leave,
}

/// A parsed command line.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Script {
    /// The commands and separators, nested pieces appended after the command that holds them.
    pub items: Vec<Item>,
}

/// Words that may stand before a command without being it.
const PREFIX: &[&str] = &["!", "{", "then", "do", "else", "elif", "if", "while", "until", "time"];
/// Shells whose `-c` argument, heredoc or here-string is shell text.
const SHELLS: &[&str] = &["bash", "sh", "zsh", "dash", "ksh"];

/// The last path component.
pub fn basename(word: &str) -> &str {
    word.rsplit('/').next().unwrap_or(word)
}

impl Simple {
    /// The words from the command itself on: assignments, `!`, reserved words (`do`, `then`, `while`...) and wrappers (`sudo`,
    /// `env`, `command`, `exec`, `nohup`, `nice`, `timeout`) with their options are skipped.
    pub fn command(&self) -> &[Word] {
        let w = &self.words;
        let mut i = 0;
        while let Some(word) = w.get(i) {
            let name = basename(&word.text);
            if word.assignment || PREFIX.contains(&word.text.as_str()) {
                i += 1;
                continue;
            }
            // Options of a wrapper that take a value in the next word.
            let (wrapper, valued): (bool, &[&str]) = match name {
                "sudo" => (true, &["-u", "-g", "-h", "-p", "-C", "-r", "-t", "-U", "-D", "-R", "-T"]),
                "env" => (true, &["-u", "-C", "-S"]),
                "nice" => (true, &["-n"]),
                "timeout" => (true, &["-s", "-k"]),
                "command" | "exec" | "nohup" | "builtin" => (true, &[]),
                _ => (false, &[]),
            };
            if !wrapper {
                break;
            }
            i += 1;
            while let Some(opt) = w.get(i) {
                if opt.assignment && name == "env" {
                    i += 1;
                } else if opt.text.starts_with('-') && opt.text.len() > 1 {
                    i += if valued.contains(&opt.text.as_str()) { 2 } else { 1 };
                } else {
                    break;
                }
            }
            if name == "timeout" {
                i += 1; // the duration
            }
        }
        w.get(i..).unwrap_or(&[])
    }

    /// The command line starts a `while` or `until` loop: its condition follows.
    pub fn opens_condition(&self) -> bool {
        for word in &self.words {
            match word.text.as_str() {
                "while" | "until" => return true,
                t if word.assignment || PREFIX.contains(&t) => {}
                _ => return false,
            }
        }
        false
    }

    /// The command line starts with the `do` of a loop.
    pub fn is_do(&self) -> bool {
        self.words.first().is_some_and(|w| w.text == "do")
    }

    /// Shell text this command runs: the argument of `bash -c`, the words of `eval`, and the input of a shell that reads a
    /// heredoc or here-string.
    fn nested_sources(&self) -> Vec<String> {
        let cmd = self.command();
        let Some(first) = cmd.first() else { return Vec::new() };
        let name = basename(&first.text);
        let mut out = Vec::new();
        if name == "eval" {
            let joined: Vec<&str> = cmd[1..].iter().map(|w| w.text.as_str()).collect();
            out.push(joined.join(" "));
        } else if SHELLS.contains(&name) {
            let mut args = cmd[1..].iter();
            let mut reads_stdin = true;
            while let Some(a) = args.next() {
                let t = a.text.as_str();
                if t == "--" {
                    break;
                }
                // `-c`, or a cluster ending in c (`-lc`, `-ec`).
                if t.starts_with('-') && !t.starts_with("--") && t.ends_with('c') {
                    if let Some(script) = args.next() {
                        out.push(script.text.clone());
                    }
                    break;
                }
                if !t.starts_with('-') {
                    reads_stdin = false; // a script file: its text is not here, and stdin is the script's
                    break;
                }
            }
            if out.is_empty() && reads_stdin {
                out.extend(self.docs.iter().cloned());
            }
        }
        out
    }
}

impl Script {
    /// Parses `src` and everything nested in it.
    pub fn parse(src: &str) -> Result<Script, LexError> {
        let mut budget = MAX_NESTED;
        let mut items = Vec::new();
        parse_into(src, 0, &mut budget, &mut items)?;
        Ok(Script { items })
    }

    /// Every simple command, in order.
    pub fn commands(&self) -> impl Iterator<Item = &Simple> {
        self.items.iter().filter_map(|i| match i {
            Item::Cmd(c) => Some(c),
            _ => None,
        })
    }
}

fn parse_into(src: &str, depth: usize, budget: &mut usize, out: &mut Vec<Item>) -> Result<(), LexError> {
    if src.len() > MAX_SOURCE || depth > MAX_DEPTH {
        return Err(LexError::TooLarge);
    }
    let (toks, bodies) = Lexer::new(src).run()?;
    let mut items = Vec::new();
    let mut cur = Simple::default();
    let mut slots: Vec<usize> = Vec::new();
    let (mut skip_target, mut here_string) = (false, false);
    let flush = |cur: &mut Simple, slots: &mut Vec<usize>, items: &mut Vec<Item>| {
        for s in slots.drain(..) {
            if let Some(b) = bodies.get(s) {
                cur.docs.push(b.clone());
            }
        }
        if !cur.words.is_empty() || !cur.docs.is_empty() || !cur.subs.is_empty() {
            items.push(Item::Cmd(std::mem::take(cur)));
        }
    };
    for tok in toks {
        match tok {
            Tok::Word(w) if skip_target => {
                // A redirection target is a file name; a substitution in it still runs.
                skip_target = false;
                cur.subs.extend(w.subs);
            }
            Tok::Word(w) if here_string => {
                here_string = false;
                cur.docs.push(w.text);
            }
            Tok::Word(w) => cur.words.push(w),
            Tok::Redir => skip_target = true,
            Tok::HereString => here_string = true,
            Tok::HereDoc(slot) => slots.push(slot),
            Tok::Sep(s) => {
                skip_target = false;
                here_string = false;
                flush(&mut cur, &mut slots, &mut items);
                items.push(Item::Sep(s));
            }
        }
    }
    flush(&mut cur, &mut slots, &mut items);
    for item in items {
        let nested: Vec<String> = match &item {
            Item::Cmd(c) => c
                .words
                .iter()
                .flat_map(|w| w.subs.iter().cloned())
                .chain(c.subs.iter().cloned())
                .chain(c.nested_sources())
                .collect(),
            _ => Vec::new(),
        };
        out.push(item);
        for n in nested {
            if *budget == 0 {
                return Err(LexError::TooLarge);
            }
            *budget -= 1;
            out.push(Item::Enter);
            parse_into(&n, depth + 1, budget, out)?;
            out.push(Item::Leave);
        }
    }
    Ok(())
}

enum Tok {
    Word(Word),
    Sep(Sep),
    Redir,
    HereString,
    HereDoc(usize),
}

struct Lexer {
    s: Vec<char>,
    i: usize,
    toks: Vec<Tok>,
    /// Heredocs whose bodies start after the next line end: (delimiter, strip leading tabs, slot).
    pending: Vec<(String, bool, usize)>,
    bodies: Vec<String>,
}

fn is_meta(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | ';' | '&' | '|' | '(' | ')' | '<' | '>')
}

impl Lexer {
    fn new(src: &str) -> Lexer {
        Lexer {
            s: src.chars().collect(),
            i: 0,
            toks: Vec::new(),
            pending: Vec::new(),
            bodies: Vec::new(),
        }
    }

    fn peek(&self, n: usize) -> Option<char> {
        self.s.get(self.i + n).copied()
    }

    fn run(mut self) -> Result<(Vec<Tok>, Vec<String>), LexError> {
        while let Some(c) = self.peek(0) {
            match c {
                ' ' | '\t' => self.i += 1,
                '\\' if self.peek(1) == Some('\n') => self.i += 2,
                '#' => {
                    while self.peek(0).is_some_and(|c| c != '\n') {
                        self.i += 1;
                    }
                }
                '\n' => {
                    self.i += 1;
                    self.toks.push(Tok::Sep(Sep::Newline));
                    self.read_bodies();
                }
                ';' => {
                    while self.peek(0) == Some(';') {
                        self.i += 1;
                    }
                    if self.peek(0) == Some('&') {
                        self.i += 1;
                    }
                    self.toks.push(Tok::Sep(Sep::Semi));
                }
                '&' => match self.peek(1) {
                    Some('&') => {
                        self.i += 2;
                        self.toks.push(Tok::Sep(Sep::And));
                    }
                    Some('>') => {
                        self.i += 2;
                        if self.peek(0) == Some('>') {
                            self.i += 1;
                        }
                        self.toks.push(Tok::Redir);
                    }
                    _ => {
                        self.i += 1;
                        self.toks.push(Tok::Sep(Sep::Amp));
                    }
                },
                '|' => {
                    self.i += 1;
                    if self.peek(0) == Some('|') {
                        self.i += 1;
                        self.toks.push(Tok::Sep(Sep::Or));
                    } else {
                        if self.peek(0) == Some('&') {
                            self.i += 1;
                        }
                        self.toks.push(Tok::Sep(Sep::Pipe));
                    }
                }
                '(' => {
                    self.i += 1;
                    self.toks.push(Tok::Sep(Sep::Open));
                }
                ')' => {
                    self.i += 1;
                    self.toks.push(Tok::Sep(Sep::Close));
                }
                '<' | '>' => self.redirection()?,
                _ => {
                    let w = self.word()?;
                    // `2>file`: a file descriptor number glued to a redirection is part of it.
                    let fd = !w.text.is_empty() && w.text.chars().all(|c| c.is_ascii_digit()) && matches!(self.peek(0), Some('<' | '>'));
                    if !fd {
                        self.toks.push(Tok::Word(w));
                    }
                }
            }
        }
        Ok((self.toks, self.bodies))
    }

    fn redirection(&mut self) -> Result<(), LexError> {
        let c = self.s[self.i];
        if self.peek(1) == Some('(') {
            // Process substitution: a word that runs a command.
            self.i += 2;
            let sub = self.balanced("process substitution")?;
            self.toks.push(Tok::Word(Word {
                text: String::new(),
                subs: vec![sub],
                assignment: false,
            }));
            return Ok(());
        }
        if c == '<' && self.peek(1) == Some('<') {
            if self.peek(2) == Some('<') {
                self.i += 3;
                self.toks.push(Tok::HereString);
                return Ok(());
            }
            self.i += 2;
            let strip = self.peek(0) == Some('-');
            if strip {
                self.i += 1;
            }
            while matches!(self.peek(0), Some(' ' | '\t')) {
                self.i += 1;
            }
            let delim = self.word()?.text;
            let slot = self.bodies.len();
            self.bodies.push(String::new());
            self.pending.push((delim, strip, slot));
            self.toks.push(Tok::HereDoc(slot));
            return Ok(());
        }
        self.i += 1;
        if matches!(self.peek(0), Some('>' | '&' | '|')) || (c == '<' && self.peek(0) == Some('>')) {
            self.i += 1;
        }
        self.toks.push(Tok::Redir);
        Ok(())
    }

    /// Reads the bodies of the pending heredocs, which start at `self.i` (just after a line end).
    fn read_bodies(&mut self) {
        for (delim, strip, slot) in std::mem::take(&mut self.pending) {
            let mut body = String::new();
            while self.i < self.s.len() {
                let start = self.i;
                while self.peek(0).is_some_and(|c| c != '\n') {
                    self.i += 1;
                }
                let line: String = self.s[start..self.i].iter().collect();
                if self.peek(0) == Some('\n') {
                    self.i += 1;
                }
                let cmp = if strip { line.trim_start_matches('\t') } else { line.as_str() };
                if cmp == delim {
                    break;
                }
                body.push_str(&line);
                body.push('\n');
            }
            if let Some(b) = self.bodies.get_mut(slot) {
                *b = body;
            }
        }
    }

    fn word(&mut self) -> Result<Word, LexError> {
        let mut w = Word::default();
        let mut plain_prefix = true; // nothing but name characters so far: an `=` now makes an assignment
        while let Some(c) = self.peek(0) {
            if is_meta(c) {
                break;
            }
            match c {
                '\'' => {
                    plain_prefix = false;
                    self.i += 1;
                    let text = self.until('\'', "single quote")?;
                    w.text.push_str(&text);
                }
                '"' => {
                    plain_prefix = false;
                    self.i += 1;
                    self.double_quoted(&mut w)?;
                }
                '\\' => {
                    plain_prefix = false;
                    self.i += 1;
                    if let Some(n) = self.peek(0) {
                        self.i += 1;
                        if n != '\n' {
                            w.text.push(n);
                        }
                    }
                }
                '$' if self.peek(1) == Some('\'') => {
                    plain_prefix = false;
                    self.i += 2;
                    let mut text = String::new();
                    loop {
                        match self.peek(0) {
                            None => return Err(LexError::Unterminated("$' quote")),
                            Some('\\') => {
                                self.i += 1;
                                if let Some(n) = self.peek(0) {
                                    text.push(n);
                                    self.i += 1;
                                }
                            }
                            Some('\'') => {
                                self.i += 1;
                                break;
                            }
                            Some(ch) => {
                                text.push(ch);
                                self.i += 1;
                            }
                        }
                    }
                    w.text.push_str(&text);
                }
                '$' | '`' => {
                    plain_prefix = false;
                    self.expansion(&mut w)?;
                }
                '=' if plain_prefix && !w.text.is_empty() => {
                    w.assignment = true;
                    plain_prefix = false;
                    w.text.push('=');
                    self.i += 1;
                }
                _ => {
                    if !(c.is_ascii_alphanumeric() || c == '_') {
                        plain_prefix = false;
                    }
                    if plain_prefix && w.text.is_empty() && c.is_ascii_digit() {
                        plain_prefix = false;
                    }
                    w.text.push(c);
                    self.i += 1;
                }
            }
        }
        Ok(w)
    }

    /// At `$` or a backtick: `$(...)` and backticks give a substitution, `${...}` and `$name` stay text.
    fn expansion(&mut self, w: &mut Word) -> Result<(), LexError> {
        match (self.peek(0), self.peek(1)) {
            (Some('`'), _) => {
                self.i += 1;
                let mut text = String::new();
                loop {
                    match self.peek(0) {
                        None => return Err(LexError::Unterminated("backtick")),
                        Some('\\') if matches!(self.peek(1), Some('`' | '\\' | '$')) => {
                            text.push(self.s[self.i + 1]);
                            self.i += 2;
                        }
                        Some('`') => {
                            self.i += 1;
                            break;
                        }
                        Some(ch) => {
                            text.push(ch);
                            self.i += 1;
                        }
                    }
                }
                w.subs.push(text);
            }
            (Some('$'), Some('(')) => {
                self.i += 2;
                let sub = self.balanced("$(")?;
                w.subs.push(sub);
            }
            (Some('$'), Some('{')) => {
                self.i += 2;
                let mut depth = 1;
                while let Some(ch) = self.peek(0) {
                    self.i += 1;
                    match ch {
                        '{' => depth += 1,
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                return Ok(());
                            }
                        }
                        _ => {}
                    }
                }
                return Err(LexError::Unterminated("${"));
            }
            _ => {
                w.text.push('$');
                self.i += 1;
            }
        }
        Ok(())
    }

    fn double_quoted(&mut self, w: &mut Word) -> Result<(), LexError> {
        loop {
            match self.peek(0) {
                None => return Err(LexError::Unterminated("double quote")),
                Some('"') => {
                    self.i += 1;
                    return Ok(());
                }
                Some('\\') => {
                    self.i += 1;
                    match self.peek(0) {
                        Some(n @ ('$' | '`' | '"' | '\\')) => {
                            w.text.push(n);
                            self.i += 1;
                        }
                        Some('\n') => self.i += 1,
                        _ => w.text.push('\\'),
                    }
                }
                Some('$' | '`') => self.expansion(w)?,
                Some(ch) => {
                    w.text.push(ch);
                    self.i += 1;
                }
            }
        }
    }

    fn until(&mut self, end: char, what: &'static str) -> Result<String, LexError> {
        let start = self.i;
        while let Some(c) = self.peek(0) {
            if c == end {
                let text = self.s[start..self.i].iter().collect();
                self.i += 1;
                return Ok(text);
            }
            self.i += 1;
        }
        Err(LexError::Unterminated(what))
    }

    /// The text up to the `)` that closes a `$(` already consumed. Quotes, escapes, nested parentheses and heredoc bodies
    /// are skipped, so a `)` or an apostrophe inside them does not end it.
    fn balanced(&mut self, what: &'static str) -> Result<String, LexError> {
        let start = self.i;
        let mut depth = 1;
        let mut pending: Vec<(String, bool)> = Vec::new();
        while let Some(c) = self.peek(0) {
            match c {
                '\'' => {
                    self.i += 1;
                    self.until('\'', what)?;
                }
                '"' => {
                    self.i += 1;
                    let mut scratch = Word::default();
                    self.double_quoted(&mut scratch)?;
                }
                '\\' => self.i += 2,
                '<' if self.peek(1) == Some('<') && self.peek(2) != Some('<') => {
                    self.i += 2;
                    let strip = self.peek(0) == Some('-');
                    if strip {
                        self.i += 1;
                    }
                    while matches!(self.peek(0), Some(' ' | '\t')) {
                        self.i += 1;
                    }
                    let delim = self.word()?.text;
                    pending.push((delim, strip));
                }
                '\n' => {
                    self.i += 1;
                    for (delim, strip) in std::mem::take(&mut pending) {
                        while self.i < self.s.len() {
                            let line_start = self.i;
                            while self.peek(0).is_some_and(|c| c != '\n') {
                                self.i += 1;
                            }
                            let line: String = self.s[line_start..self.i].iter().collect();
                            if self.peek(0) == Some('\n') {
                                self.i += 1;
                            }
                            let cmp = if strip { line.trim_start_matches('\t') } else { line.as_str() };
                            if cmp == delim {
                                break;
                            }
                        }
                    }
                }
                '(' => {
                    depth += 1;
                    self.i += 1;
                }
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        let text = self.s[start..self.i].iter().collect();
                        self.i += 1;
                        return Ok(text);
                    }
                    self.i += 1;
                }
                _ => self.i += 1,
            }
        }
        Err(LexError::Unterminated(what))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmds(src: &str) -> Vec<Vec<String>> {
        Script::parse(src)
            .unwrap()
            .commands()
            .map(|c| c.words.iter().map(|w| w.text.clone()).collect())
            .collect()
    }

    #[test]
    fn words_split_on_blanks_and_lose_their_quotes() {
        assert_eq!(
            cmds(r#"git commit -m "a b" 'c d' e\ f"#),
            [["git", "commit", "-m", "a b", "c d", "e f"]]
        );
        assert_eq!(cmds("echo $'a\\'b' x"), vec![vec!["echo", "a'b", "x"]]);
    }

    #[test]
    fn separators_split_commands_and_redirections_vanish() {
        assert_eq!(
            cmds("a && b || c; d | e & f\ng"),
            vec![vec!["a"], vec!["b"], vec!["c"], vec!["d"], vec!["e"], vec!["f"], vec!["g"]]
        );
        assert_eq!(cmds("ls >out 2>&1 -l &>x"), vec![vec!["ls", "-l"]]);
        assert_eq!(cmds("a 2>/dev/null; b >> f"), vec![vec!["a"], vec!["b"]]);
    }

    #[test]
    fn comments_end_at_the_line_and_a_hash_inside_a_word_is_text() {
        assert_eq!(cmds("a # b c\nd x#y"), vec![vec!["a"], vec!["d", "x#y"]]);
    }

    #[test]
    fn assignments_are_marked_only_for_an_unquoted_name() {
        let s = Script::parse(r#"A=1 B="x y" 'C=2' =3 1D=4 cmd"#).unwrap();
        let w: Vec<bool> = s.commands().next().unwrap().words.iter().map(|w| w.assignment).collect();
        assert_eq!(w, [true, true, false, false, false, false]);
    }

    #[test]
    fn heredoc_bodies_are_data_not_commands() {
        let s = Script::parse("cat <<'EOF'\ngit push -f\nEOF\nls").unwrap();
        assert_eq!(cmds("cat <<'EOF'\ngit push -f\nEOF\nls"), vec![vec!["cat"], vec!["ls"]]);
        assert_eq!(s.commands().next().unwrap().docs, ["git push -f\n"]);
        assert_eq!(cmds("cat <<-X\n\tbody\n\tX\nls"), vec![vec!["cat"], vec!["ls"]]);
    }

    #[test]
    fn two_heredocs_on_one_line_read_in_order() {
        let s = Script::parse("cmd <<A <<B\none\nA\ntwo\nB\nnext").unwrap();
        assert_eq!(s.commands().next().unwrap().docs, ["one\n", "two\n"]);
    }

    #[test]
    fn a_heredoc_without_its_end_runs_to_the_end_of_the_input() {
        let s = Script::parse("cat <<EOF\nabc").unwrap();
        assert_eq!(s.commands().next().unwrap().docs, ["abc\n"]);
    }

    #[test]
    fn substitutions_are_parsed_and_come_after_their_command() {
        assert_eq!(
            cmds("echo $(git push -f) `date`"),
            vec![vec!["echo", "", ""], vec!["git", "push", "-f"], vec!["date"]]
        );
        assert_eq!(cmds(r#"echo "x $(a b) y""#), vec![vec!["echo", "x  y"], vec!["a", "b"]]);
        assert_eq!(cmds("echo '$(a b)'"), vec![vec!["echo", "$(a b)"]]);
        assert_eq!(cmds("diff <(a x) b"), vec![vec!["diff", "", "b"], vec!["a", "x"]]);
    }

    #[test]
    fn a_substitution_in_a_redirection_target_runs() {
        assert_eq!(cmds("echo hi > $(mk x)"), vec![vec!["echo", "hi"], vec!["mk", "x"]]);
    }

    #[test]
    fn a_closing_parenthesis_or_apostrophe_in_a_heredoc_inside_a_substitution_does_not_end_it() {
        let src = "git commit -m \"$(cat <<'EOF'\ndon't) break\nEOF\n)\" && ls";
        assert_eq!(cmds(src).last().unwrap(), &["ls"]);
        assert!(cmds(src).iter().any(|c| c[0] == "git"));
    }

    #[test]
    fn shells_and_eval_are_parsed_again() {
        assert_eq!(
            cmds("bash -c 'a b; c'"),
            vec![vec!["bash", "-c", "a b; c"], vec!["a", "b"], vec!["c"]]
        );
        assert_eq!(cmds("sh -lc 'x y'")[1], ["x", "y"]);
        assert_eq!(cmds("eval git push -f")[1], ["git", "push", "-f"]);
        assert_eq!(cmds("/bin/bash -c 'x'")[1], ["x"]);
        // not a shell, not parsed
        assert_eq!(cmds("echo -c 'x y'"), vec![vec!["echo", "-c", "x y"]]);
    }

    #[test]
    fn a_shell_fed_a_heredoc_or_here_string_runs_it() {
        assert_eq!(cmds("bash <<EOF\nx y\nEOF")[1], ["x", "y"]);
        assert_eq!(cmds("sh <<< 'x y'")[1], ["x", "y"]);
        assert_eq!(cmds("cat <<< 'x y'"), vec![vec!["cat"]]);
        assert_eq!(cmds("bash script.sh <<EOF\nx y\nEOF"), vec![vec!["bash", "script.sh"]]);
    }

    #[test]
    fn command_skips_assignments_reserved_words_and_wrappers() {
        let c = |s: &str| {
            let sc = Script::parse(s).unwrap();
            sc.commands()
                .next()
                .unwrap()
                .command()
                .iter()
                .map(|w| w.text.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(c("FOO=1 git push"), ["git", "push"]);
        assert_eq!(c("do sleep 1"), ["sleep", "1"]);
        assert_eq!(c("while ! pgrep x"), ["pgrep", "x"]);
        assert_eq!(c("sudo -u me git push"), ["git", "push"]);
        assert_eq!(c("env -i A=1 -u B git x"), ["git", "x"]);
        assert_eq!(c("timeout 5 git x"), ["git", "x"]);
        assert_eq!(c("nohup nice -n 5 time git x"), ["git", "x"]);
        assert_eq!(c("command -v git"), ["git"]);
        assert!(c("FOO=1").is_empty());
    }

    #[test]
    fn loops_are_recognised() {
        let s = Script::parse("while true; do sleep 1; done").unwrap();
        let v: Vec<(bool, bool)> = s.commands().map(|c| (c.opens_condition(), c.is_do())).collect();
        assert_eq!(v, [(true, false), (false, true), (false, false)]);
        assert!(Script::parse("until x").unwrap().commands().next().unwrap().opens_condition());
        assert!(!Script::parse("echo while").unwrap().commands().next().unwrap().opens_condition());
    }

    #[test]
    fn odd_input_is_an_error_not_a_panic() {
        for bad in ["echo 'a", "echo \"a", "echo $(a", "echo `a", "echo ${a", "echo <(a", "echo $'a"] {
            assert!(matches!(Script::parse(bad), Err(LexError::Unterminated(_))), "{bad}");
        }
        for ok in ["", "   ", "\n\n", ";;", "&&", "|", ")", "a )", "> ", "echo \\", "$", "cat <<"] {
            assert!(Script::parse(ok).is_ok(), "{ok:?}");
        }
    }

    #[test]
    fn limits_hold() {
        assert_eq!(Script::parse(&"a ".repeat(MAX_SOURCE)), Err(LexError::TooLarge));
        let deep = "$(".repeat(20) + "x" + &")".repeat(20);
        assert_eq!(Script::parse(&format!("echo {deep}")), Err(LexError::TooLarge));
        let wide = (0..100).map(|_| "$(a)").collect::<Vec<_>>().join(" ");
        assert_eq!(Script::parse(&format!("echo {wide}")), Err(LexError::TooLarge));
    }

    #[test]
    fn utf8_text_is_kept() {
        assert_eq!(cmds("echo héllo \"日本\""), vec![vec!["echo", "héllo", "日本"]]);
    }
}
