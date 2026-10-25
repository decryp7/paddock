//! How a watch tells releases apart: which one is the latest, and (for a pattern with an `id` group) which products
//! it tracks and how each is named, muted and soaked. All keys are optional with defaults, so older files load.

use crate::filter::MAX_SOAK_HOURS;
use crate::release::LatestBy;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Most products a watch lists in its settings.
pub const MAX_PRODUCT_SETTINGS: usize = 200;
/// Longest product display name, in characters.
pub const MAX_PRODUCT_NAME: usize = 80;

/// Per-product overrides.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ProductSetting {
    /// Display name (`PRO` shown as "Acme Installer Pro"); empty for the id itself.
    #[serde(default)]
    pub name: String,
    /// No notifications for this product (it is still tracked and shown).
    #[serde(default)]
    pub muted: bool,
    /// Soak time for this product; `None` uses the watch's.
    #[serde(default)]
    pub soak_hours: Option<u32>,
}

/// The products of a watch.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ProductsSpec {
    /// Track only these ids; empty for every product found.
    #[serde(default)]
    pub only: Vec<String>,
    /// Announce a product that appears after the first check as a new release (off: it is the new baseline).
    #[serde(default)]
    pub announce_new: bool,
    /// Overrides by product id.
    #[serde(default)]
    pub settings: BTreeMap<String, ProductSetting>,
}

impl ProductsSpec {
    /// The name to show for a product.
    pub fn display_name<'a>(&'a self, id: &'a str) -> &'a str {
        self.settings.get(id).map(|s| s.name.trim()).filter(|n| !n.is_empty()).unwrap_or(id)
    }

    /// Whether notifications for the product are off.
    pub fn is_muted(&self, id: &str) -> bool {
        self.settings.get(id).is_some_and(|s| s.muted)
    }

    /// The soak time for a product given the watch's.
    pub fn soak_hours(&self, id: &str, watch_default: u32) -> u32 {
        self.settings.get(id).and_then(|s| s.soak_hours).unwrap_or(watch_default)
    }

    /// Checks the lists; the message says what to fix.
    pub fn check(&self) -> Result<(), String> {
        if self.only.len() > MAX_PRODUCT_SETTINGS || self.settings.len() > MAX_PRODUCT_SETTINGS {
            return Err(format!("A watch can list at most {MAX_PRODUCT_SETTINGS} products."));
        }
        if let Some(bad) = self.settings.values().find(|s| s.name.chars().count() > MAX_PRODUCT_NAME) {
            return Err(format!(
                "The product name \"{}…\" is too long (at most {MAX_PRODUCT_NAME} characters).",
                bad.name.chars().take(20).collect::<String>()
            ));
        }
        if self.settings.values().any(|s| s.soak_hours.is_some_and(|h| h > MAX_SOAK_HOURS)) {
            return Err(format!("A product's soak time can be at most {MAX_SOAK_HOURS} hours (30 days)."));
        }
        Ok(())
    }
}

/// Which release is the latest, and which products are tracked.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Tracking {
    /// How the newest release is decided.
    #[serde(default)]
    pub latest_by: LatestBy,
    /// A number group the name leaves out counts as older than any number (default: as zero).
    #[serde(default)]
    pub missing_older: bool,
    /// Products.
    #[serde(default)]
    pub products: ProductsSpec,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn products_are_named_muted_and_soaked() {
        let mut p = ProductsSpec::default();
        p.settings.insert(
            "PRO".into(),
            ProductSetting {
                name: "Acme Installer Pro".into(),
                muted: true,
                soak_hours: Some(6),
            },
        );
        assert_eq!(p.display_name("PRO"), "Acme Installer Pro");
        assert_eq!(p.display_name("QA"), "QA");
        assert!(p.is_muted("PRO") && !p.is_muted("QA"));
        assert_eq!((p.soak_hours("PRO", 0), p.soak_hours("QA", 2)), (6, 2));
        assert!(p.check().is_ok());
        p.settings.get_mut("PRO").unwrap().soak_hours = Some(MAX_SOAK_HOURS + 1);
        assert!(p.check().unwrap_err().contains("soak time"));
        p.settings.get_mut("PRO").unwrap().soak_hours = None;
        p.settings.get_mut("PRO").unwrap().name = "x".repeat(MAX_PRODUCT_NAME + 1);
        assert!(p.check().unwrap_err().contains("too long"));
        p.only = (0..=MAX_PRODUCT_SETTINGS).map(|i| i.to_string()).collect();
        assert!(p.check().unwrap_err().contains("at most"));
    }

    #[test]
    fn older_files_without_the_keys_load_with_defaults() {
        let t: Tracking = toml::from_str("").unwrap();
        assert_eq!(t, Tracking::default());
        let t: Tracking =
            toml::from_str("latest_by = \"modified\"\n[products]\nonly = [\"PRO\"]\n[products.settings.PRO]\nname = \"Pro\"").unwrap();
        assert_eq!(t.latest_by, LatestBy::Modified);
        assert_eq!(t.products.display_name("PRO"), "Pro");
    }
}
