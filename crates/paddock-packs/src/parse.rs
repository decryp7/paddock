use crate::{Limits, Origin, PackError, PackKind};
use toml::Value;

/// A parsed pack and the notes about it.
#[derive(Debug, Clone, PartialEq)]
pub struct Parsed<P> {
    /// The app's value.
    pub pack: P,
    /// Keys the kind does not know, as `unknown key "a.b" is ignored`.
    pub warnings: Vec<String>,
}

/// Parses one pack file: size, TOML, `format`, limits, then the typed read with unknown keys collected, then the kind's
/// own rules. Nothing in a pack is executed or followed (no includes, no paths opened). `Err` refuses the whole pack.
pub fn parse_pack<K: PackKind>(
    bytes: &[u8],
    origin: Origin,
    limits: &Limits,
) -> Result<Parsed<K::Pack>, PackError> {
    let len = bytes.len() as u64;
    if len > limits.max_file_bytes {
        return Err(PackError::TooLarge {
            len,
            max: limits.max_file_bytes,
        });
    }
    let text = std::str::from_utf8(bytes).map_err(|_| PackError::NotText)?;
    let mut table: toml::Table = toml::from_str(text)
        .map_err(|e| PackError::Syntax(e.message().to_string() + &at(&e, text)))?;
    let format = match table.remove("format") {
        Some(Value::Integer(n)) => n,
        Some(_) => {
            return Err(PackError::Format(
                "\"format\" must be a whole number".into(),
            ));
        }
        None => {
            return Err(PackError::Format(format!(
                "\"format\" is missing; this version reads format {}",
                K::FORMAT
            )));
        }
    };
    if format != i64::from(K::FORMAT) {
        let which = if format > i64::from(K::FORMAT) {
            "a newer version of the app"
        } else {
            "an older format"
        };
        return Err(PackError::Format(format!(
            "\"format\" is {format} ({which}); this version reads format {}",
            K::FORMAT
        )));
    }
    limits.check(&table)?;
    let mut warnings = Vec::new();
    let raw: K::Raw = serde_ignored::deserialize(Value::Table(table), |p| {
        warnings.push(format!("unknown key \"{p}\" is ignored"))
    })
    .map_err(|e: toml::de::Error| PackError::Shape(e.message().to_string()))?;
    let pack = K::convert(raw, origin)?;
    Ok(Parsed { pack, warnings })
}

/// " (line 3, column 5)" for a syntax error, empty when the parser has no span.
fn at(e: &toml::de::Error, text: &str) -> String {
    e.span()
        .map(|span| {
            let before = text.get(..span.start).unwrap_or_default();
            let line = before.matches('\n').count() + 1;
            let col = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
            format!(" (line {line}, column {col})")
        })
        .unwrap_or_default()
}
