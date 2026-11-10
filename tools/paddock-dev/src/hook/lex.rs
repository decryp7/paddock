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
/// The deepest nesting of `$(...)`, `bash -c` and `eval`; past a limit the rules check the raw text instead (`hook::decide`).
const MAX_DEPTH: usize = 16;
/// The most nested pieces parsed for one command.
const MAX_NESTED: usize = 10_000;

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
    /// Any part of it was quoted or escaped (`'a'`, `"a"`, `\a`, `$'a'`).
    pub quoted: bool,
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

    /// The command line starts a `for` or `select` loop (after `do`, `then`, `{`...).
    pub fn opens_for(&self) -> bool {
        for word in &self.words {
            match word.text.as_str() {
                "for" | "select" => return true,
                "while" | "until" => return false,
                t if word.assignment || PREFIX.contains(&t) => {}
                _ => return false,
            }
        }
        false
    }

    /// Shell text this command runs: the argument of `eval`, of a shell's `-c` (a shell may stand after any word: `xargs sh -c`,
    /// `find -exec bash -c`, `nohup sh -c`, `ssh host bash -c`), of `env -S`, and the input of a shell that reads a heredoc or
    /// here-string.
    fn nested_sources(&self) -> Vec<String> {
        let mut out = Vec::new();
        let w = &self.words;
        let cmd = self.command();
        if let Some(first) = cmd.first()
            && basename(&first.text) == "eval"
        {
            let joined: Vec<&str> = cmd[1..].iter().map(|w| w.text.as_str()).collect();
            out.push(joined.join(" "));
        }
        let mut shell_reads_stdin = false;
        for (k, word) in w.iter().enumerate() {
            let name = basename(&word.text);
            if SHELLS.contains(&name) {
                match shell_script(&w[k + 1..]) {
                    ShellInput::Script(text) => out.push(text),
                    ShellInput::Stdin => shell_reads_stdin = true,
                    ShellInput::File => {}
                }
            } else if name == "env" {
                let mut args = w[k + 1..].iter();
                while let Some(a) = args.next() {
                    let t = a.text.as_str();
                    if t == "-S" || t == "--split-string" {
                        if let Some(script) = args.next() {
                            out.push(script.text.clone());
                        }
                    } else if let Some(rest) = t.strip_prefix("--split-string=") {
                        out.push(rest.to_string());
                    } else if let Some(rest) = t.strip_prefix("-S").filter(|r| !r.is_empty()) {
                        out.push(rest.to_string());
                    }
                }
            }
        }
        if out.is_empty() && shell_reads_stdin {
            out.extend(self.docs.iter().cloned());
        }
        out
    }
}

/// What a shell is given.
enum ShellInput {
    /// `-c SCRIPT`.
    Script(String),
    /// No script and no file: it reads standard input (a heredoc or here-string).
    Stdin,
    /// A script file: its text is not in the command line, and standard input is the script's.
    File,
}

/// The input of a shell whose arguments (after its name) are `args`.
fn shell_script(args: &[Word]) -> ShellInput {
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let t = a.text.as_str();
        if t == "--" {
            return ShellInput::Stdin;
        }
        // `-c`, or a cluster ending in c (`-lc`, `-ec`).
        if t.starts_with('-') && !t.starts_with("--") && t.ends_with('c') {
            return it.next().map_or(ShellInput::Stdin, |s| ShellInput::Script(s.text.clone()));
        }
        if !t.starts_with('-') {
            return ShellInput::File;
        }
    }
    ShellInput::Stdin
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
    let (toks, bodies, body_subs) = Lexer::new(src).run()?;
    let mut items = Vec::new();
    let mut cur = Simple::default();
    let mut slots: Vec<usize> = Vec::new();
    let (mut skip_target, mut here_string) = (false, false);
    let flush = |cur: &mut Simple, slots: &mut Vec<usize>, items: &mut Vec<Item>| {
        for s in slots.drain(..) {
            if let Some(b) = bodies.get(s) {
                cur.docs.push(b.clone());
            }
            if let Some(subs) = body_subs.get(s) {
                cur.subs.extend(subs.iter().cloned());
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
                cur.subs.extend(w.subs);
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

/// The tokens, the heredoc bodies and each body's substitutions.
type Lexed = (Vec<Tok>, Vec<String>, Vec<Vec<String>>);

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
    /// Heredocs whose bodies start after the next line end: (delimiter, strip leading tabs, slot, expansions run).
    pending: Vec<(String, bool, usize, bool)>,
    bodies: Vec<String>,
    /// The substitutions of each unquoted heredoc body, by slot.
    body_subs: Vec<Vec<String>>,
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
            body_subs: Vec::new(),
        }
    }

    fn peek(&self, n: usize) -> Option<char> {
        self.s.get(self.i + n).copied()
    }

    fn run(mut self) -> Result<Lexed, LexError> {
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
                '(' if self.peek(1) == Some('(') => {
                    // `(( ... ))`: arithmetic, where `<<` is a shift and not a heredoc. It must close with `))`; `((a); b)` is two
                    // nested subshells (as bash reads it) and is lexed as parentheses.
                    let start = self.i;
                    self.i += 2;
                    match self.arith() {
                        Ok(subs) if self.i >= 2 && self.s[self.i - 2] == ')' && self.s[self.i - 1] == ')' => {
                            self.toks.push(Tok::Word(Word {
                                text: "((".to_string(),
                                subs,
                                ..Word::default()
                            }));
                        }
                        _ => {
                            self.i = start + 1;
                            self.toks.push(Tok::Sep(Sep::Open));
                        }
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
        Ok((self.toks, self.bodies, self.body_subs))
    }

    fn redirection(&mut self) -> Result<(), LexError> {
        let c = self.s[self.i];
        if self.peek(1) == Some('(') {
            // Process substitution: a word that runs a command.
            self.i += 2;
            let sub = self.balanced("process substitution")?;
            self.toks.push(Tok::Word(Word {
                subs: vec![sub],
                ..Word::default()
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
            let delim_word = self.word()?;
            let slot = self.bodies.len();
            self.bodies.push(String::new());
            self.body_subs.push(Vec::new());
            self.pending.push((delim_word.text, strip, slot, !delim_word.quoted));
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
        for (delim, strip, slot, expand) in std::mem::take(&mut self.pending) {
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
            if expand && let Some(subs) = self.body_subs.get_mut(slot) {
                *subs = Lexer::new(&body).substitutions_in_text();
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
                    w.quoted = true;
                    self.i += 1;
                    let text = self.until('\'', "single quote")?;
                    w.text.push_str(&text);
                }
                '"' => {
                    plain_prefix = false;
                    w.quoted = true;
                    self.i += 1;
                    self.double_quoted(&mut w)?;
                }
                '\\' => {
                    plain_prefix = false;
                    w.quoted = true;
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
                    w.quoted = true;
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
                    self.expansion(&mut w, false)?;
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
    fn expansion(&mut self, w: &mut Word, in_dq: bool) -> Result<(), LexError> {
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
            (Some('$'), Some('(')) if self.peek(2) == Some('(') => {
                // `$(( ... ))`: arithmetic.
                self.i += 3;
                let subs = self.arith()?;
                w.subs.extend(subs);
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
                        '\\' => self.i += 1,
                        // Outside double quotes the default word of `${x:-...}` has quotes of its own; inside, a quote is text.
                        '\'' if !in_dq => {
                            self.until('\'', "single quote")?;
                        }
                        '"' if !in_dq => {
                            let mut scratch = Word::default();
                            self.double_quoted(&mut scratch)?;
                            w.subs.extend(scratch.subs);
                        }
                        '$' if self.peek(0) == Some('\'') && !in_dq => {
                            self.i += 1;
                            self.ansi_c_skip()?;
                        }
                        '$' if self.peek(0) == Some('(') => {
                            self.i += 1;
                            let sub = self.balanced("$(")?;
                            w.subs.push(sub);
                        }
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
                Some('$' | '`') => self.expansion(w, true)?,
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

    /// After `$'`: skips to the closing quote; a backslash escapes the next character (`\'`).
    fn ansi_c_skip(&mut self) -> Result<(), LexError> {
        while let Some(c) = self.peek(0) {
            self.i += 1;
            match c {
                '\\' => self.i += 1,
                '\'' => return Ok(()),
                _ => {}
            }
        }
        Err(LexError::Unterminated("$' quote"))
    }

    /// After `((` or `$((`: skips to the `))` that closes it, counting parentheses. Returns the `$(...)` substitutions inside,
    /// which run.
    fn arith(&mut self) -> Result<Vec<String>, LexError> {
        let mut depth = 2;
        let mut subs = Vec::new();
        while let Some(c) = self.peek(0) {
            match c {
                '\\' => self.i += 2,
                '(' => {
                    depth += 1;
                    self.i += 1;
                }
                ')' => {
                    depth -= 1;
                    self.i += 1;
                    if depth == 0 {
                        return Ok(subs);
                    }
                }
                '$' if self.peek(1) == Some('(') && self.peek(2) != Some('(') => {
                    self.i += 2;
                    subs.push(self.balanced("$(")?);
                }
                _ => self.i += 1,
            }
        }
        Err(LexError::Unterminated("arithmetic"))
    }

    /// The `$(...)` and backtick substitutions in the text of an unquoted heredoc body (an escaped `$` or backtick is text). An
    /// unclosed one ends the scan: the body is data to every other reader.
    fn substitutions_in_text(mut self) -> Vec<String> {
        let mut subs = Vec::new();
        while let Some(c) = self.peek(0) {
            match c {
                '\\' => self.i += 2,
                '$' if self.peek(1) == Some('(') && self.peek(2) == Some('(') => {
                    self.i += 3;
                    match self.arith() {
                        Ok(inner) => subs.extend(inner),
                        Err(_) => break,
                    }
                }
                '$' if self.peek(1) == Some('(') => {
                    self.i += 2;
                    match self.balanced("$(") {
                        Ok(sub) => subs.push(sub),
                        Err(_) => break,
                    }
                }
                '`' => {
                    self.i += 1;
                    let mut text = String::new();
                    loop {
                        match self.peek(0) {
                            None => return subs,
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
                    subs.push(text);
                }
                _ => self.i += 1,
            }
        }
        subs
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
                '$' if self.peek(1) == Some('\'') => {
                    self.i += 2;
                    self.ansi_c_skip()?;
                }
                '(' if self.peek(1) == Some('(') => {
                    // arithmetic: `<<` inside is a shift
                    self.i += 2;
                    self.arith()?;
                }
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
        let wide = (0..10_001).map(|_| "$(a)").collect::<Vec<_>>().join(" ");
        assert_eq!(Script::parse(&format!("echo {wide}")), Err(LexError::TooLarge));
    }

    #[test]
    fn utf8_text_is_kept() {
        assert_eq!(cmds("echo héllo \"日本\""), vec![vec!["echo", "héllo", "日本"]]);
    }

    #[test]
    fn arithmetic_is_not_a_heredoc() {
        assert_eq!(cmds("(( y = 1<<2 ))\ngit push -f"), vec![vec!["(("], vec!["git", "push", "-f"]]);
        assert_eq!(cmds("echo $(( 1<<2 )); ls"), vec![vec!["echo", ""], vec!["ls"]]);
        assert_eq!(cmds("x=$((1<<3)); ls"), vec![vec!["x="], vec!["ls"]]);
        assert_eq!(
            cmds("while (( 1<<2 )); do :; done"),
            vec![vec!["while", "(("], vec!["do", ":"], vec!["done"]]
        );
        // a substitution inside arithmetic still runs
        assert!(cmds("(( x = $(a b) ))").contains(&vec!["a".to_string(), "b".to_string()]));
        // inside a substitution too
        assert_eq!(cmds("echo $(echo $(( 1<<2 )) ); ls").last().unwrap(), &["ls"]);
        // `((a); b)` is two nested subshells, not arithmetic: what is inside runs
        assert!(cmds("((git push -f) | cat)").contains(&vec!["git".to_string(), "push".to_string(), "-f".to_string()]));
        assert!(cmds("((a b); c d)").contains(&vec!["c".to_string(), "d".to_string()]));
        assert!(Script::parse("(( 1 + 2").is_ok());
    }

    #[test]
    fn quotes_inside_a_parameter_expansion_are_honoured() {
        assert_eq!(cmds("echo ${x:-\"}\"}; ls"), vec![vec!["echo", ""], vec!["ls"]]);
        assert_eq!(cmds("echo ${y:-'}'}; ls"), vec![vec!["echo", ""], vec!["ls"]]);
        assert_eq!(cmds("echo ${z:-\\}}; ls"), vec![vec!["echo", ""], vec!["ls"]]);
        assert_eq!(cmds("echo ${a:-${b:-x}}; ls"), vec![vec!["echo", ""], vec!["ls"]]);
        // in double quotes a single quote is text
        assert_eq!(cmds("echo \"${x:-'}\"; ls"), vec![vec!["echo", ""], vec!["ls"]]);
        // a substitution in the default runs
        assert!(cmds("echo ${x:-$(a b)}").contains(&vec!["a".to_string(), "b".to_string()]));
        assert!(matches!(Script::parse("echo ${x:-\"}"), Err(LexError::Unterminated(_))));
    }

    #[test]
    fn ansi_c_quotes_inside_a_substitution_do_not_end_it_early() {
        assert_eq!(cmds("echo $(echo $'\\''); ls").last().unwrap(), &["ls"]);
        assert_eq!(cmds("echo $(echo $'a)b'); ls").last().unwrap(), &["ls"]);
    }

    #[test]
    fn the_limits_are_wide_enough_for_ordinary_nesting() {
        let many = vec!["$(:)"; 65].join(" ");
        assert_eq!(cmds(&format!("echo {many}; ls")).last().unwrap(), &["ls"]);
        let mut nest = "git push -f".to_string();
        for _ in 0..8 {
            nest = format!("bash -c '{}'", nest.replace('\'', "'\\''"));
        }
        assert!(cmds(&nest).iter().any(|c| c.first().map(String::as_str) == Some("git")));
        let too_many = vec!["$(:)"; 10_001].join(" ");
        assert_eq!(Script::parse(&format!("echo {too_many}")), Err(LexError::TooLarge));
    }

    #[test]
    fn a_shell_is_found_after_any_command_word() {
        let has = |src: &str, want: &[&str]| cmds(src).iter().any(|c| c == want);
        assert!(has("echo x | xargs -I{} sh -c 'a b {}'", &["a", "b", "{}"]));
        assert!(has("find . -maxdepth 0 -exec bash -c 'c d' \\;", &["c", "d"]));
        assert!(has("nohup sh -c 'e f' &", &["e", "f"]));
        assert!(has("env -S 'g h'", &["g", "h"]));
        assert!(has("env -S'i j'", &["i", "j"]));
        assert!(has("env --split-string='k l'", &["k", "l"]));
        assert!(has("ssh host bash -lc 'm n'", &["m", "n"]));
        // a word that is not a shell, or a shell given a script file, runs nothing nested
        assert_eq!(cmds("echo sh"), vec![vec!["echo", "sh"]]);
        assert_eq!(cmds("grep sh file"), vec![vec!["grep", "sh", "file"]]);
        assert_eq!(cmds("xargs bash script.sh"), vec![vec!["xargs", "bash", "script.sh"]]);
    }

    #[test]
    fn here_strings_and_unquoted_heredocs_run_their_substitutions() {
        let has = |src: &str, want: &[&str]| cmds(src).iter().any(|c| c == want);
        assert!(has("cat <<< \"$(a b)\"", &["a", "b"]));
        assert!(has("cat <<< $(a b)", &["a", "b"]));
        assert!(has("cat <<EOF\nx $(c d) y\nEOF", &["c", "d"]));
        assert!(has("cat <<EOF\nx `e f` y\nEOF", &["e", "f"]));
        assert!(has("cat <<-EOF\n\tx $(g h)\n\tEOF", &["g", "h"]));
        // a quoted delimiter means no expansion; an escaped dollar is text
        for src in [
            "cat <<'EOF'\n$(c d)\nEOF",
            "cat <<\"EOF\"\n$(c d)\nEOF",
            "cat <<\\EOF\n$(c d)\nEOF",
            "cat <<EOF\n\\$(c d)\nEOF",
        ] {
            assert!(!has(src, &["c", "d"]), "{src}");
        }
        // a heredoc fed to a shell is still run as a script
        assert!(has("bash <<EOF\nx y\nEOF", &["x", "y"]));
    }

    #[test]
    fn words_know_whether_they_were_quoted() {
        let s = Script::parse("a 'b' \"c\" d\\e $'f' g").unwrap();
        let q: Vec<bool> = s.commands().next().unwrap().words.iter().map(|w| w.quoted).collect();
        assert_eq!(q, [false, true, true, true, true, false]);
    }

    #[test]
    fn for_loops_are_recognised_as_loops() {
        let c = |s: &str| Script::parse(s).unwrap().commands().next().unwrap().opens_for();
        assert!(c("for i in 1 2; do :; done"));
        assert!(c("do for i in 1; do :; done"));
        assert!(c("select x in a b"));
        assert!(!c("echo for"));
        assert!(!c("while true"));
    }
}
