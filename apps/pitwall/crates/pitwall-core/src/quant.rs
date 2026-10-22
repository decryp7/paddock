//! Quantisation names (`Q4_K_M`, `IQ3_XS`, `BF16`, …): read from file names and headers, and how many bytes per
//! parameter they cost (ported from IMT's `recommend::Quant`).

use serde::Serialize;

/// A quantisation family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Quant {
    /// 2-bit.
    Q2,
    /// 3-bit.
    Q3,
    /// 4-bit.
    Q4,
    /// 5-bit.
    Q5,
    /// 6-bit.
    Q6,
    /// 8-bit.
    Q8,
    /// 16-bit float (F16, BF16).
    F16,
    /// 32-bit float.
    F32,
}

impl Quant {
    /// GB of weights per billion parameters.
    pub fn gb_per_b(self) -> f64 {
        match self {
            Quant::Q2 => 0.4,
            Quant::Q3 => 0.5,
            Quant::Q4 => 0.6,
            Quant::Q5 => 0.7,
            Quant::Q6 => 0.85,
            Quant::Q8 => 1.1,
            Quant::F16 => 2.0,
            Quant::F32 => 4.0,
        }
    }

    /// Understands "Q4_K_M", "Q8_0", "F16", "BF16", "IQ4_XS", ...
    pub fn parse(s: &str) -> Option<Quant> {
        let u = s.trim().to_uppercase();
        let u = u.strip_prefix("IQ").map(|r| format!("Q{r}")).unwrap_or(u);
        match u.as_str() {
            "F16" | "BF16" => return Some(Quant::F16),
            "F32" => return Some(Quant::F32),
            _ => {}
        }
        match u.strip_prefix('Q')?.chars().next()? {
            '2' => Some(Quant::Q2),
            '3' => Some(Quant::Q3),
            '4' => Some(Quant::Q4),
            '5' => Some(Quant::Q5),
            '6' => Some(Quant::Q6),
            '8' => Some(Quant::Q8),
            _ => None,
        }
    }
}

/// Whether a `-`/`.`-separated token of a file name is a quantisation label (`Q4_K_M`, `IQ3_XS`, `F16`).
fn is_label(t: &str) -> bool {
    let u = t.to_ascii_uppercase();
    let head = u.split('_').next().unwrap_or_default();
    matches!(head, "F16" | "BF16" | "F32")
        || head
            .strip_prefix("IQ")
            .or_else(|| head.strip_prefix('Q'))
            .is_some_and(|r| r.starts_with(|c: char| c.is_ascii_digit()))
}

/// The quantisation label in a GGUF file name (`Qwen3-8B-Q4_K_M.gguf` → `Q4_K_M`), upper-cased; the last matching
/// token wins.
pub fn label_from_file_name(file: &str) -> Option<String> {
    let base = file.rsplit(['/', '\\']).next().unwrap_or(file);
    let stem = base.strip_suffix(".gguf").unwrap_or(base);
    let stem = crate::ids::strip_split_suffix(stem);
    stem.rsplit(['-', '.']).find(|t| is_label(t)).map(str::to_ascii_uppercase)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsing() {
        for (s, q) in [
            ("Q4_K_M", Quant::Q4),
            ("q8_0", Quant::Q8),
            ("F16", Quant::F16),
            ("BF16", Quant::F16),
            ("IQ3_XS", Quant::Q3),
            ("Q6_K", Quant::Q6),
            ("Q5_K_S", Quant::Q5),
            ("Q2_K", Quant::Q2),
            ("F32", Quant::F32),
        ] {
            assert_eq!(Quant::parse(s), Some(q), "{s}");
            assert!(q.gb_per_b() > 0.0);
        }
        assert_eq!(Quant::parse("weird"), None);
        assert_eq!(Quant::parse("Q9"), None);
        assert_eq!(Quant::parse(""), None);
    }

    #[test]
    fn labels_from_file_names() {
        assert_eq!(label_from_file_name("Qwen3-8B-Q4_K_M.gguf").as_deref(), Some("Q4_K_M"));
        assert_eq!(label_from_file_name("org/gemma-3-12b-it-IQ3_XS.gguf").as_deref(), Some("IQ3_XS"));
        assert_eq!(label_from_file_name("model.Q8_0.gguf").as_deref(), Some("Q8_0"));
        assert_eq!(label_from_file_name("model-bf16-00001-of-00002.gguf").as_deref(), Some("BF16"));
        assert_eq!(label_from_file_name("mmproj-model-f16.gguf").as_deref(), Some("F16"));
        assert_eq!(label_from_file_name("plain.gguf"), None);
        assert_eq!(label_from_file_name("q-x.gguf"), None);
    }
}
