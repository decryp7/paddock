use crate::PackError;
use toml::Value;

/// The bounds on one pack file. Defaults suit the three apps' packs; an app tightens them for its kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Largest file read, in bytes (a larger file is refused unread).
    pub max_file_bytes: u64,
    /// Most elements in any one list or array of tables.
    pub max_entries: usize,
    /// Longest string, in bytes, as a key or a value.
    pub max_string_len: usize,
    /// Deepest nesting of tables and arrays.
    pub max_depth: usize,
    /// Most values in the whole file.
    pub max_nodes: usize,
    /// Most `*.toml` files read from the user's folder (the rest are reported, not read).
    pub max_packs: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_file_bytes: 512 * 1024,
            max_entries: 500,
            max_string_len: 4096,
            max_depth: 12,
            max_nodes: 50_000,
            max_packs: 64,
        }
    }
}

impl Limits {
    /// Walks the parsed tree once and refuses the first thing over a limit. Iterative, so a deep file cannot overflow
    /// the stack even though depth is checked as it goes.
    pub(crate) fn check(&self, root: &toml::Table) -> Result<(), PackError> {
        let mut nodes = 0usize;
        let mut stack: Vec<(&Value, usize, String)> = Vec::new();
        for (k, v) in root {
            self.check_str(k, "a key")?;
            stack.push((v, 1, k.clone()));
        }
        while let Some((v, depth, path)) = stack.pop() {
            nodes += 1;
            if nodes > self.max_nodes {
                return Err(PackError::Limit(format!(
                    "the pack has more than {} values",
                    self.max_nodes
                )));
            }
            if depth > self.max_depth {
                return Err(PackError::Limit(format!(
                    "\"{path}\" is nested more than {} levels deep",
                    self.max_depth
                )));
            }
            match v {
                Value::String(s) if s.len() > self.max_string_len => {
                    return Err(PackError::Limit(format!(
                        "\"{path}\" is {} bytes long; the limit for a string is {}",
                        s.len(),
                        self.max_string_len
                    )));
                }
                Value::Array(a) => {
                    if a.len() > self.max_entries {
                        return Err(PackError::Limit(format!(
                            "\"{path}\" has {} entries; the limit is {}",
                            a.len(),
                            self.max_entries
                        )));
                    }
                    for (i, x) in a.iter().enumerate() {
                        stack.push((x, depth + 1, format!("{path}[{i}]")));
                    }
                }
                Value::Table(t) => {
                    for (k, x) in t {
                        self.check_str(k, &format!("a key in \"{path}\""))?;
                        stack.push((x, depth + 1, format!("{path}.{k}")));
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn check_str(&self, s: &str, what: &str) -> Result<(), PackError> {
        if s.len() > self.max_string_len {
            return Err(PackError::Limit(format!(
                "{what} is {} bytes long; the limit is {}",
                s.len(),
                self.max_string_len
            )));
        }
        Ok(())
    }
}
