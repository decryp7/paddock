//! A rule's own good and bad examples are its test: every `bad` example must be flagged and no `good` example may be.

use pacenotes_core::RepoPath;
use pacenotes_parse::ParseLanguage;

use crate::pattern::{CompiledRule, MatchLimits, example_extension};

/// One example that did not behave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    /// The language the example was run as.
    pub language: &'static str,
    /// `good` or `bad`.
    pub kind: &'static str,
    /// The example's position in its list (1-based).
    pub number: usize,
    /// What went wrong and what to do.
    pub problem: String,
}

/// The result of testing one rule.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RuleTestReport {
    /// Examples that passed.
    pub passed: usize,
    /// Examples that did not.
    pub failures: Vec<Failure>,
}

/// Runs a rule over its own examples, in each of its languages. (A structural rule without a good and a bad example is refused when the
/// spec is read, so there is always something to run.)
pub fn test_rule(rule: &CompiledRule) -> RuleTestReport {
    let mut report = RuleTestReport::default();
    let spec = rule.spec();
    let languages: Vec<ParseLanguage> = rule.languages().collect();
    for language in languages {
        let Ok(path) = RepoPath::parse(&format!("example.{}", example_extension(language))) else {
            continue;
        };
        for (kind, examples, want) in [("bad", spec.bad(), true), ("good", spec.good(), false)] {
            for (i, example) in examples.iter().enumerate() {
                let found = !rule.run(&path, example.as_bytes(), &MatchLimits::default()).is_empty();
                if found == want {
                    report.passed += 1;
                } else {
                    let problem = if want {
                        format!(
                            "the pattern of rule {} does not flag this `bad` example; change the pattern or the example",
                            spec.id().as_str()
                        )
                    } else {
                        format!(
                            "the pattern of rule {} flags this `good` example; narrow the pattern or fix the example",
                            spec.id().as_str()
                        )
                    };
                    report.failures.push(Failure {
                        language: language.id(),
                        kind,
                        number: i + 1,
                        problem,
                    });
                }
            }
        }
    }
    report
}
