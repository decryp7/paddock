# 10. `pacenotes-xml`: the one safe way to read untrusted XML

Status: proposed (2026-10-30), from the reviews of CV1 and RQ1. Numbered at merge.

## Context

Two crates read XML that somebody else wrote: coverage reports (`pacenotes-cover`: Cobertura, JaCoCo) and office files
(`pacenotes-req`: the parts of an xlsx or docx). Each had its own guard, and each guard failed on a reviewer's input: a top-level entity
bomb (`<!DOCTYPE c [<!ENTITY a "AAAA...">]>` and ten thousand `&a;`: 5 GB, over a minute), a `>` inside an attribute value that hid
5,000 levels of nesting from a depth scan (stack overflow, abort), and 5,000 nested `<w:p>` in a 523-byte docx (stack overflow). Shared
Paddock crates are frozen, so the one entry point lives in Pacenotes (ER-22: one place, tested once).

## Decision

`pacenotes-xml` (`clean`, `parse`, `with_document`, `line_of`), used before every parse:

1. **A `DOCTYPE` with a `[` is refused.** An internal subset is where entities are declared; refusing it is what stops expansion. A
   bare `DOCTYPE` (JaCoCo's names an external DTD) is **blanked with spaces**, same length and lines, so nothing is fetched and error
   lines stay true. The parser also runs with `allow_dtd` off.
2. **Nesting is counted before the parser runs**, by a scan that understands quoted attribute values (both quote characters),
   comments, CDATA, processing instructions and self-closing tags, and refuses past 256 levels with the line.
3. **The parser's node limit** bounds the tree.
4. **Errors say where**: the line of a refusal or the parser's own position, and `line_of(doc, node)` gives importers the line of a
   node for their own messages.

What it does **not** promise: it is not a validating parser, and it does not bound time for a document that is merely very large
(the callers bound bytes and nodes). The earlier ADRs 0008 and 0009 said the XML reader "never loads an external entity or DTD" and "never
expands entities"; that was true of the parser's defaults and false of the guard around it, and they now point here.

## Consequences

- Both importers share the tests of this crate (the reviewers' inputs) and add their own through their public API.
- A document that legitimately needs entities (none of the formats read here) would be refused; that is the intended trade.
