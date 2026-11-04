//! What a platform's tray draws of a number item: the formats it has and whether the label fits in the icon. The engine decides
//! (the UI renders what this says and holds no per-OS rule).

use super::menu_bar::NumberFormat;
use crate::tray::pixel;
use serde::{Deserialize, Serialize};

/// The platform whose tray draws the items.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayPlatform {
    /// The macOS menu bar: templates 18 pt high, the width follows the format.
    Mac,
    /// The Windows notification area: square icons of the taskbar's pixel size.
    Windows,
    /// The Linux appindicator: 22 px square icons.
    Linux,
}

impl TrayPlatform {
    /// The platform this build runs on.
    pub const fn current() -> TrayPlatform {
        if cfg!(target_os = "macos") {
            TrayPlatform::Mac
        } else if cfg!(target_os = "windows") {
            TrayPlatform::Windows
        } else {
            TrayPlatform::Linux
        }
    }
}

/// What this platform draws of a number item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "ts"), derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct NumberSupport {
    /// The formats the tray draws; the others are shown but cannot be chosen.
    pub formats: Vec<NumberFormat>,
    /// Whether the label is drawn in the icon (it is always in the tooltip).
    pub label_in_icon: bool,
    /// Why a format or the label is missing, in one sentence for the Settings page; `None` when everything is drawn.
    pub note: Option<String>,
}

/// What the tray draws of a number item on `platform`, for icons of `px` pixels (the taskbar's size; unused on macOS).
pub fn number_support(platform: TrayPlatform, px: u32) -> NumberSupport {
    match platform {
        TrayPlatform::Mac => NumberSupport {
            formats: vec![NumberFormat::Stacked, NumberFormat::Line],
            label_in_icon: true,
            note: None,
        },
        TrayPlatform::Windows | TrayPlatform::Linux => {
            let label_in_icon = pixel::layout(i32::try_from(px).unwrap_or(i32::MAX)).label_rows > 0;
            NumberSupport {
                formats: vec![NumberFormat::Stacked],
                label_in_icon,
                note: Some(if label_in_icon {
                    "Windows and Linux tray icons have one number layout: the label over the value. The label is also in the tooltip."
                        .into()
                } else {
                    format!("Tray icons are {px} px here, too small for a label above the number. The label shows in the tooltip; a larger display scaling makes room for it.")
                }),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mac_menu_bar_draws_both_formats_and_the_label() {
        let s = number_support(TrayPlatform::Mac, 18);
        assert_eq!(s.formats, [NumberFormat::Stacked, NumberFormat::Line]);
        assert!(s.label_in_icon && s.note.is_none());
    }

    #[test]
    fn small_windows_icons_have_no_room_for_a_label_and_say_why() {
        for px in [16, 20] {
            let s = number_support(TrayPlatform::Windows, px);
            assert!(!s.label_in_icon, "{px}");
            assert_eq!(s.formats, [NumberFormat::Stacked], "{px}");
            let note = s.note.expect("a reason");
            assert!(note.contains(&format!("{px} px")) && note.contains("tooltip"), "{note}");
        }
    }

    #[test]
    fn larger_windows_icons_draw_the_label_and_agree_with_the_renderer() {
        for px in [16, 20, 22, 24, 32] {
            let s = number_support(TrayPlatform::Windows, px);
            assert_eq!(s.label_in_icon, pixel::layout(px as i32).label_rows > 0, "{px}");
        }
        assert!(number_support(TrayPlatform::Windows, 24).label_in_icon);
        assert!(number_support(TrayPlatform::Linux, 22).label_in_icon);
    }

    /// The claim "the label is drawn" is the renderer's: a label changes the pixels above the digits.
    #[test]
    fn the_label_is_drawn_exactly_where_the_support_says() {
        use pixel::{render, Kind, Reading, State, Theme};
        let r = Reading {
            value: Some(54.0),
            ..Reading::default()
        };
        for px in [16, 20, 22, 24, 32] {
            let l = pixel::layout(px);
            let with = |label: &str| render(&Kind::Percent { label: label.into() }, &r, State::Normal, px, Theme { dark: true });
            let differs = with("CPU").rgba != with("MEM").rgba;
            let top = l.digits_y.max(1) as usize * px as usize * 4;
            let ink_above = with("CPU").rgba[..top].chunks(4).any(|p| p[3] > 0);
            assert_eq!(differs, number_support(TrayPlatform::Windows, px as u32).label_in_icon, "{px}");
            assert_eq!(ink_above, differs, "{px}");
        }
    }
}
