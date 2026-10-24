//! Secrets by reference (ADR 0005). The crate never resolves a secret: [`Resolver`] is implemented by Sighter (OS keychain).
//! Writers take a [`Redactor`] and replace resolved values by `{{secret:NAME}}`, so reports never contain them.

use crate::ids::SecretRef;
use crate::plan::Plan;

/// Looks a secret up by name. Implemented by the app that runs the plan; this crate only calls it from [`SecretRedactor::for_plan`].
pub trait Resolver {
    /// The secret's value, or `None` when it is not stored.
    fn resolve(&self, name: &SecretRef) -> Option<String>;
}

/// Removes secret values from free text before it is written.
pub trait Redactor {
    /// The text with every known secret value replaced by its `{{secret:NAME}}` placeholder.
    fn redact(&self, text: &str) -> String;
}

/// Redacts nothing: for plans and results that cannot hold resolved values, and for tests.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoRedaction;

impl Redactor for NoRedaction {
    fn redact(&self, text: &str) -> String {
        text.to_owned()
    }
}

/// Replaces the resolved values of a set of secrets. Longer values win where they overlap, empty values are ignored.
#[derive(Debug, Clone, Default)]
pub struct SecretRedactor {
    entries: Vec<(SecretRef, String)>,
}

impl SecretRedactor {
    /// From `(name, value)` pairs.
    pub fn new(entries: impl IntoIterator<Item = (SecretRef, String)>) -> Self {
        Self {
            entries: entries.into_iter().filter(|(_, v)| !v.is_empty()).collect(),
        }
    }

    /// Resolves every secret the plan refers to and keeps those that are stored.
    pub fn for_plan(plan: &Plan, resolver: &dyn Resolver) -> Self {
        Self::new(
            plan.secret_refs()
                .into_iter()
                .filter_map(|n| resolver.resolve(&n).map(|v| (n, v))),
        )
    }
}

impl Redactor for SecretRedactor {
    fn redact(&self, text: &str) -> String {
        let mut spans: Vec<(usize, usize, usize)> = Vec::new();
        for (i, (_, value)) in self.entries.iter().enumerate() {
            spans.extend(
                text.match_indices(value.as_str())
                    .map(|(at, m)| (at, at + m.len(), i)),
            );
        }
        spans.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
        let mut out = String::with_capacity(text.len());
        let mut at = 0;
        for (start, end, i) in spans {
            if start < at {
                continue;
            }
            out.push_str(&text[at..start]);
            out.push_str(&format!("{{{{secret:{}}}}}", self.entries[i].0));
            at = end;
        }
        out.push_str(&text[at..]);
        out
    }
}
