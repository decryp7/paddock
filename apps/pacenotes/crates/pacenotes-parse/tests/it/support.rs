//! Helpers: render an outline as text, so a golden is readable.

use pacenotes_parse::{Limits, Outline, ParseLanguage, parse};
use paddock_jobs::CancelToken;

pub fn outline(language: ParseLanguage, source: &str) -> Outline {
    parse(language, source.as_bytes(), &Limits::default(), &CancelToken::new()).unwrap()
}

/// One line per item: `symbol <kind> <qualified> <first line>-<last line>`, `import <target>`, `call <caller> -> <callee>`,
/// `chunk <symbol|-> <ordinal> <first line>-<last line>`.
pub fn render(o: &Outline) -> String {
    let mut lines = Vec::new();
    let last = |s: &pacenotes_core::Span| {
        let e = s.end();
        if e.column() == 1 && e.line() > s.start().line() {
            e.line() - 1
        } else {
            e.line()
        }
    };
    for s in &o.symbols {
        lines.push(format!(
            "symbol {:?} {} {}-{}",
            s.kind,
            s.qualified_name,
            s.span.start().line(),
            last(&s.span)
        ));
    }
    for i in &o.imports {
        lines.push(format!("import {}", i.target));
    }
    for c in &o.calls {
        lines.push(format!("call {} -> {}", c.caller.as_deref().unwrap_or("-"), c.callee));
    }
    for c in &o.chunks {
        lines.push(format!(
            "chunk {} {} {}-{}",
            c.symbol.as_deref().unwrap_or("-"),
            c.ordinal,
            c.span.start().line(),
            last(&c.span)
        ));
    }
    lines.join("\n")
}
