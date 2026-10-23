//! `\Object(Instance)\Counter` parsed once into a typed value.

use super::MAX_PATH;
use std::fmt;
use std::str::FromStr;

/// Why a counter path is not usable (the message says what to change).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CounterError {
    /// Empty, too long or containing control characters.
    #[error("A counter path is 1 to {MAX_PATH} characters without control characters.")]
    BadText,
    /// Not of the form `\Object\Counter` or `\Object(Instance)\Counter`.
    #[error("Write the counter as \\Object\\Counter or \\Object(Instance)\\Counter.")]
    BadShape,
    /// A wildcard: every counter is read on its own, so name one instance.
    #[error("Wildcards are not supported; name one instance.")]
    Wildcard,
    /// Too many paths in one request or set.
    #[error("At most {0} counters at a time.")]
    TooMany(usize),
}

/// A parsed counter path. `Display` gives the canonical text, which parses back to the same value.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CounterPath {
    object: String,
    instance: Option<String>,
    counter: String,
}

impl CounterPath {
    /// A path from its parts; the same checks as parsing.
    pub fn new(object: &str, instance: Option<&str>, counter: &str) -> Result<CounterPath, CounterError> {
        if [Some(object), instance, Some(counter)]
            .into_iter()
            .flatten()
            .any(|s| s.contains(['*', '?']))
        {
            return Err(CounterError::Wildcard);
        }
        let part = |s: &str| {
            let t = s.trim();
            (!t.is_empty() && !t.contains('\\') && !t.chars().any(char::is_control)).then(|| t.to_string())
        };
        let object = part(object).ok_or(CounterError::BadShape)?;
        let counter = part(counter).ok_or(CounterError::BadShape)?;
        let instance = match instance {
            Some(i) if i.trim().is_empty() => return Err(CounterError::BadShape),
            Some(i) => Some(part(i).ok_or(CounterError::BadShape)?),
            None => None,
        };
        let p = CounterPath { object, instance, counter };
        if p.to_string().chars().count() > MAX_PATH {
            return Err(CounterError::BadText);
        }
        Ok(p)
    }

    /// The object (`Memory`, `Network Interface`).
    pub fn object(&self) -> &str {
        &self.object
    }

    /// The instance (`en0`, `_Total`), when the object has instances.
    pub fn instance(&self) -> Option<&str> {
        self.instance.as_deref()
    }

    /// The counter (`Bytes Received`).
    pub fn counter(&self) -> &str {
        &self.counter
    }
}

impl fmt::Display for CounterPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.instance {
            Some(i) => write!(f, "\\{}({})\\{}", self.object, i, self.counter),
            None => write!(f, "\\{}\\{}", self.object, self.counter),
        }
    }
}

impl FromStr for CounterPath {
    type Err = CounterError;

    fn from_str(text: &str) -> Result<CounterPath, CounterError> {
        let text = text.trim();
        if text.is_empty() || text.chars().count() > MAX_PATH || text.chars().any(char::is_control) {
            return Err(CounterError::BadText);
        }
        let rest = text.strip_prefix('\\').ok_or(CounterError::BadShape)?;
        // The counter is after the last backslash; the instance (which may itself contain parentheses, as in
        // `Network Interface(Intel(R) Ethernet)`) is inside the outermost parentheses of what is before it.
        let (head, counter) = rest.rsplit_once('\\').ok_or(CounterError::BadShape)?;
        match head.find('(') {
            None => CounterPath::new(head, None, counter),
            Some(open) => {
                let inner = head[open + 1..].strip_suffix(')').ok_or(CounterError::BadShape)?;
                CounterPath::new(&head[..open], Some(inner), counter)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_parses_with_and_without_an_instance_and_round_trips() {
        let p: CounterPath = "\\Memory\\Pages/sec".parse().unwrap();
        assert_eq!((p.object(), p.instance(), p.counter()), ("Memory", None, "Pages/sec"));
        assert_eq!(p.to_string(), "\\Memory\\Pages/sec");
        let p: CounterPath = " \\Processor Information(_Total)\\% Processor Time ".parse().unwrap();
        assert_eq!(p.instance(), Some("_Total"));
        assert_eq!(p.to_string().parse::<CounterPath>().unwrap(), p);
    }

    #[test]
    fn nested_parentheses_in_an_instance_are_kept() {
        let p: CounterPath = "\\Network Interface(Intel(R) Ethernet (2))\\Bytes Total/sec".parse().unwrap();
        assert_eq!(p.object(), "Network Interface");
        assert_eq!(p.instance(), Some("Intel(R) Ethernet (2)"));
        assert_eq!(p.counter(), "Bytes Total/sec");
    }

    #[test]
    fn bad_shapes_say_what_to_write() {
        for bad in ["Memory\\Pages", "\\Memory", "\\\\x", "\\(a)\\b", "\\M()\\c", "\\M(a\\c", "\\M\\"] {
            assert_eq!(bad.parse::<CounterPath>(), Err(CounterError::BadShape), "{bad}");
        }
        assert!(CounterError::BadShape.to_string().contains("\\Object(Instance)\\Counter"));
    }

    #[test]
    fn bad_text_is_refused() {
        assert_eq!("".parse::<CounterPath>(), Err(CounterError::BadText));
        assert_eq!("\\a\\b\u{7}".parse::<CounterPath>(), Err(CounterError::BadText));
        let long = format!("\\{}\\x", "a".repeat(MAX_PATH));
        assert_eq!(long.parse::<CounterPath>(), Err(CounterError::BadText));
        assert!(CounterPath::new("a", None, &"b".repeat(MAX_PATH)).is_err());
        assert!(CounterPath::new("a", Some(" "), "b").is_err());
        assert_eq!(
            "\\Processor(*)\\% Processor Time".parse::<CounterPath>(),
            Err(CounterError::Wildcard)
        );
        assert!(CounterError::Wildcard.to_string().contains("one instance"));
        assert!(CounterPath::new("a", Some("x\u{1}"), "b").is_err());
    }
}
