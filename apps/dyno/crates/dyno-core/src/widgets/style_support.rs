//! What a platform's tray offers and draws of the menu-bar styles: the styles to choose from (a number's formats among them
//! where the tray draws both), the metrics that draw one picture whatever the style, and what to tell the person after an add.
//! The engine decides (the UI renders what this says and holds no per-OS rule or wording). Every style is drawn in a picture of
//! its own on every platform; the app's tray tests hold the renderers to that (`tray_look`: every pair of styles differs in at
//! least a quarter of its pixels).

use super::menu_bar::{MenuBarCatalogue, MenuBarKind, NumberFormat};
use super::metric::Metric;
use super::number_support::{number_formats, TrayPlatform};
use paddock_widgets::Catalogue;
use serde::{Deserialize, Serialize};

/// One entry of the style picker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "ts"), derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct StyleChoice {
    /// The style (the item's kind).
    pub kind: MenuBarKind,
    /// For a number where the tray draws both formats, the format this entry sets; `None` for every other entry.
    pub format: Option<NumberFormat>,
    /// What the picker shows ("Line graph", "Number, one line").
    pub label: String,
}

/// A metric whose item draws one picture whatever its style, and how its row is named.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "ts"), derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct FixedStyle {
    /// The metric.
    pub metric: Metric,
    /// The row's title, in place of the style's ("Upload").
    pub title: String,
    /// The row's summary, in place of the style's.
    pub summary: String,
    /// The row's icon, in place of the style's (a widget icon name: "network", "temperature").
    pub icon: String,
    /// What its item draws, in words for the row's style field ("Rate with an arrow").
    pub drawn: String,
}

/// What this platform's tray offers and draws of the menu-bar styles.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "ts"), derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct StyleSupport {
    /// The style picker's entries, in the catalogue's order (a number's formats in [`NumberFormat`] order).
    pub styles: Vec<StyleChoice>,
    /// The metrics whose picture does not follow the style.
    pub fixed: Vec<FixedStyle>,
    /// Said once after an item is added: where the new icon appears and how to keep it in view; `None` when it simply
    /// appears.
    pub added_note: Option<String>,
}

impl StyleSupport {
    /// The entry of `metric`'s row when its picture does not follow the style.
    pub fn fixed_for(&self, metric: &Metric) -> Option<&FixedStyle> {
        self.fixed.iter().find(|f| &f.metric == metric)
    }
}

/// Where a new Windows 11 notification-area icon goes, and how to keep it on the taskbar. Windows puts every new icon in the
/// overflow; only the person can promote it (Dyno does not write the shell's promotion flag).
pub const WINDOWS_ADDED_NOTE: &str = "Windows puts a new icon in the overflow: open it with the ^ (Show hidden icons) on the \
taskbar. To keep the icon on the taskbar, turn it on in Settings > Personalization > Taskbar > Other system tray icons, or drag \
it from the overflow onto the taskbar.";

/// The summary of a row whose metric draws one picture.
const FIXED_SUMMARY: &str = "One picture in every style";

/// What the tray of `platform` offers and draws (at every icon size: no style changes with the taskbar's scaling).
pub fn style_support(platform: TrayPlatform) -> StyleSupport {
    let formats = number_formats(platform);
    let styles = MenuBarCatalogue::kinds()
        .iter()
        .flat_map(|k| {
            if k.kind == MenuBarKind::Number && formats.len() > 1 {
                formats
                    .iter()
                    .map(|f| StyleChoice {
                        kind: k.kind,
                        format: Some(*f),
                        label: match f {
                            NumberFormat::Stacked => k.title.to_string(),
                            NumberFormat::Line => format!("{}, one line", k.title),
                        },
                    })
                    .collect::<Vec<_>>()
            } else {
                vec![StyleChoice {
                    kind: k.kind,
                    format: None,
                    label: k.title.to_string(),
                }]
            }
        })
        .collect();
    let fixed = |metric: Metric, title: &str, icon: &str, drawn: &str| FixedStyle {
        metric,
        title: title.into(),
        summary: FIXED_SUMMARY.into(),
        icon: icon.into(),
        drawn: drawn.into(),
    };
    StyleSupport {
        styles,
        fixed: vec![
            fixed(Metric::NetworkUp, "Upload", "network", "Rate with an arrow"),
            fixed(Metric::NetworkDown, "Download", "network", "Rate with an arrow"),
            fixed(Metric::NetworkBoth, "Network", "network", "Both rates with arrows"),
            fixed(Metric::Temperature, "Temperature", "temperature", "Degrees"),
        ],
        added_note: (platform == TrayPlatform::Windows).then(|| WINDOWS_ADDED_NOTE.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_style_is_offered_in_the_catalogue_order() {
        for platform in [TrayPlatform::Mac, TrayPlatform::Windows, TrayPlatform::Linux] {
            let s = style_support(platform);
            let mut kinds: Vec<MenuBarKind> = s.styles.iter().map(|d| d.kind).collect();
            kinds.dedup();
            let catalogue: Vec<MenuBarKind> = MenuBarCatalogue::kinds().iter().map(|k| k.kind).collect();
            assert_eq!(kinds, catalogue, "{platform:?}");
            assert!(s.styles.iter().all(|d| !d.label.is_empty()));
        }
    }

    #[test]
    fn a_number_is_two_styles_where_the_tray_draws_both_formats_and_one_elsewhere() {
        let numbers = |p| {
            style_support(p)
                .styles
                .into_iter()
                .filter(|d| d.kind == MenuBarKind::Number)
                .map(|d| (d.format, d.label))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            numbers(TrayPlatform::Mac),
            [
                (Some(NumberFormat::Stacked), "Number".to_string()),
                (Some(NumberFormat::Line), "Number, one line".to_string())
            ]
        );
        for p in [TrayPlatform::Windows, TrayPlatform::Linux] {
            assert_eq!(numbers(p), [(None, "Number".to_string())], "{p:?}");
        }
    }

    #[test]
    fn network_and_temperature_keep_their_picture_and_name_their_row() {
        for platform in [TrayPlatform::Mac, TrayPlatform::Windows, TrayPlatform::Linux] {
            let s = style_support(platform);
            for m in [Metric::NetworkUp, Metric::NetworkDown, Metric::NetworkBoth, Metric::Temperature] {
                let f = s.fixed_for(&m).unwrap_or_else(|| panic!("{platform:?} {m:?}"));
                assert!(!f.title.is_empty() && !f.summary.is_empty() && !f.drawn.is_empty() && !f.icon.is_empty());
            }
            assert!(s.fixed_for(&Metric::CpuTotal).is_none());
            assert_eq!(s.fixed_for(&Metric::NetworkUp).unwrap().title, "Upload");
        }
    }

    #[test]
    fn only_windows_says_where_a_new_icon_goes_and_how_to_pin_it() {
        let note = style_support(TrayPlatform::Windows).added_note.expect("a note on Windows");
        assert!(note.contains("overflow") && note.contains("Show hidden icons"), "{note}");
        assert!(
            note.contains("Settings > Personalization > Taskbar > Other system tray icons"),
            "{note}"
        );
        assert!(!note.contains("clock"), "the chevron is not next to the clock: {note}");
        assert_eq!(style_support(TrayPlatform::Mac).added_note, None);
        assert_eq!(style_support(TrayPlatform::Linux).added_note, None);
    }

    /// The mock's imitation of this function is checked against these values (`style-support-golden.spec.ts`): the file is
    /// checked on every test run and `PADDOCK_WRITE_TS=1` rewrites it.
    #[test]
    fn the_generated_golden_values_are_current() {
        use paddock_commands::testing::{self, WriteMode};
        use std::path::Path;
        let case = |name: &str, platform| format!("  {name}: {},\n", serde_json::to_string(&style_support(platform)).expect("json"));
        let body = format!(
            "// Generated from `style_support` (dyno-core). Do not edit: PADDOCK_WRITE_TS=1 cargo test -p dyno-core --lib widgets::style_support.\n\nexport const STYLE_SUPPORT_GOLDEN = {{\n{}{}{}}} as const;\n",
            case("macos", TrayPlatform::Mac),
            case("windows", TrayPlatform::Windows),
            case("linux", TrayPlatform::Linux),
        );
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../app/src/app/core/generated/style-support-golden.ts");
        testing::sync_generated(&path, &body, WriteMode::from_env()).unwrap_or_else(|e| panic!("{e}"));
    }
}
