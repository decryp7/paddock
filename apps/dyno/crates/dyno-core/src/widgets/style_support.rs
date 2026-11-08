//! What a platform's tray draws of each menu-bar style: which styles have a picture of their own, which are drawn as
//! another style there, which metrics draw one picture whatever the style, and what to tell the person after an add. The
//! engine decides (the UI renders what this says and holds no per-OS rule); the app's tray tests check every claim against
//! the pictures its renderers draw (`tray_look`: equal pictures exactly where this says "drawn as").

use super::menu_bar::{MenuBarCatalogue, MenuBarKind};
use super::metric::Metric;
use super::number_support::TrayPlatform;
use paddock_widgets::Catalogue;
use serde::{Deserialize, Serialize};

/// How one style is drawn here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "ts"), derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct StyleDrawn {
    /// The style.
    pub kind: MenuBarKind,
    /// The style whose picture it shares here; `None` when it has its own.
    pub drawn_as: Option<MenuBarKind>,
    /// What the style picker shows: the style's title, and ", drawn as …" when it shares a picture.
    pub label: String,
}

/// A metric whose item draws one picture whatever its style.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "ts"), derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct FixedStyle {
    /// The metric.
    pub metric: Metric,
    /// What its item draws, in words for the row ("Rate with arrows").
    pub drawn: String,
}

/// What this platform's tray draws of the menu-bar styles.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "ts"), derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct StyleSupport {
    /// Every style of the catalogue, in its order.
    pub styles: Vec<StyleDrawn>,
    /// The metrics whose picture does not follow the style.
    pub fixed: Vec<FixedStyle>,
    /// Said once after an item is added: where the new icon appears and how to keep it in view; `None` when it simply
    /// appears.
    pub added_note: Option<String>,
}

impl StyleSupport {
    /// The style `kind` is drawn as here (itself when it has its own picture).
    pub fn drawn_as(&self, kind: MenuBarKind) -> MenuBarKind {
        self.styles.iter().find(|s| s.kind == kind).and_then(|s| s.drawn_as).unwrap_or(kind)
    }

    /// Whether an item of `metric` draws the same picture in every style.
    pub fn is_fixed(&self, metric: &Metric) -> bool {
        self.fixed.iter().any(|f| &f.metric == metric)
    }
}

/// Where a new Windows notification-area icon goes, and how to keep it on the taskbar. Windows 11 puts every new icon in the
/// overflow (behind the chevron); only the person can promote it (Dyno does not write the shell's promotion flag).
pub const WINDOWS_ADDED_NOTE: &str = "Windows puts a new icon in the overflow, behind the ^ next to the clock. To keep it on the \
taskbar, open Settings > Personalization > Taskbar > Other system tray icons and turn it on, or drag it from the overflow \
onto the taskbar.";

/// The style each style is drawn as on `platform`: macOS draws the ring as the meter (a 270-degree gauge read as a spinner
/// at 18 pt); Windows and Linux draw every style in its own picture at every size (16, 20, 22, 24, 32 px).
fn shared_picture(platform: TrayPlatform, kind: MenuBarKind) -> Option<MenuBarKind> {
    match (platform, kind) {
        (TrayPlatform::Mac, MenuBarKind::Ring) => Some(MenuBarKind::HorizontalMeter),
        _ => None,
    }
}

fn title(kind: MenuBarKind) -> &'static str {
    MenuBarCatalogue::kinds().iter().find(|k| k.kind == kind).map_or("", |k| k.title)
}

/// What the tray of `platform` draws of each style (at every icon size: no style changes with the taskbar's scaling).
pub fn style_support(platform: TrayPlatform) -> StyleSupport {
    let styles = MenuBarCatalogue::kinds()
        .iter()
        .map(|k| {
            let drawn_as = shared_picture(platform, k.kind);
            let label = match drawn_as {
                Some(other) => format!("{}, drawn as {}", k.title, title(other).to_lowercase()),
                None => k.title.to_string(),
            };
            StyleDrawn {
                kind: k.kind,
                drawn_as,
                label,
            }
        })
        .collect();
    let rate = "Rate with arrows";
    let fixed = vec![
        FixedStyle {
            metric: Metric::NetworkUp,
            drawn: rate.into(),
        },
        FixedStyle {
            metric: Metric::NetworkDown,
            drawn: rate.into(),
        },
        FixedStyle {
            metric: Metric::NetworkBoth,
            drawn: "Both rates with arrows".into(),
        },
        FixedStyle {
            metric: Metric::Temperature,
            drawn: "Degrees".into(),
        },
    ];
    StyleSupport {
        styles,
        fixed,
        added_note: (platform == TrayPlatform::Windows).then(|| WINDOWS_ADDED_NOTE.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_style_is_listed_in_the_catalogue_order() {
        for platform in [TrayPlatform::Mac, TrayPlatform::Windows, TrayPlatform::Linux] {
            let s = style_support(platform);
            let kinds: Vec<MenuBarKind> = s.styles.iter().map(|d| d.kind).collect();
            let catalogue: Vec<MenuBarKind> = MenuBarCatalogue::kinds().iter().map(|k| k.kind).collect();
            assert_eq!(kinds, catalogue, "{platform:?}");
        }
    }

    #[test]
    fn the_mac_ring_is_drawn_as_the_meter_and_says_so() {
        let s = style_support(TrayPlatform::Mac);
        assert_eq!(s.drawn_as(MenuBarKind::Ring), MenuBarKind::HorizontalMeter);
        let ring = s.styles.iter().find(|d| d.kind == MenuBarKind::Ring).unwrap();
        assert_eq!(ring.label, "Ring, drawn as meter");
        let shared = s.styles.iter().filter(|d| d.drawn_as.is_some()).count();
        assert_eq!(shared, 1, "only the ring shares a picture on macOS");
    }

    #[test]
    fn windows_and_linux_draw_every_style_in_its_own_picture() {
        for platform in [TrayPlatform::Windows, TrayPlatform::Linux] {
            let s = style_support(platform);
            assert!(s.styles.iter().all(|d| d.drawn_as.is_none()), "{platform:?}");
            assert!(s.styles.iter().all(|d| !d.label.contains("drawn as")), "{platform:?}");
        }
    }

    #[test]
    fn network_and_temperature_keep_their_picture_on_every_platform() {
        for platform in [TrayPlatform::Mac, TrayPlatform::Windows, TrayPlatform::Linux] {
            let s = style_support(platform);
            for m in [Metric::NetworkUp, Metric::NetworkDown, Metric::NetworkBoth, Metric::Temperature] {
                assert!(s.is_fixed(&m), "{platform:?} {m:?}");
            }
            assert!(!s.is_fixed(&Metric::CpuTotal));
            assert!(s.fixed.iter().all(|f| !f.drawn.is_empty()));
        }
    }

    #[test]
    fn only_windows_says_where_a_new_icon_goes_and_how_to_pin_it() {
        let note = style_support(TrayPlatform::Windows).added_note.expect("a note on Windows");
        assert!(note.contains("overflow"), "{note}");
        assert!(
            note.contains("Settings > Personalization > Taskbar > Other system tray icons"),
            "{note}"
        );
        assert_eq!(style_support(TrayPlatform::Mac).added_note, None);
        assert_eq!(style_support(TrayPlatform::Linux).added_note, None);
    }
}
