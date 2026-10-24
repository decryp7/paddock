//! Steps and targets (ADR 0005): the allowed actions are exactly the closed [`Step`] enum.

use std::fmt;

use crate::error::PlanError;
use crate::ids::{Column, ScreenName, SecretRef, clip};

/// The class of control a [`Target`] names, aligned with Sighter's detector classes. `Other` is the escape hatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ControlClass {
    /// A push button.
    Button,
    /// A hyperlink.
    Link,
    /// A single- or multi-line text input.
    TextBox,
    /// A check box.
    CheckBox,
    /// A radio button.
    RadioButton,
    /// A combo box or drop-down.
    ComboBox,
    /// A list item.
    ListItem,
    /// A menu item.
    MenuItem,
    /// A tab.
    Tab,
    /// A tree item.
    TreeItem,
    /// A table or grid.
    Table,
    /// A static label.
    Label,
    /// An image.
    Image,
    /// A window or dialog frame.
    Window,
    /// Anything else.
    Other,
}

impl ControlClass {
    const ALL: [(Self, &'static str); 15] = [
        (Self::Button, "button"),
        (Self::Link, "link"),
        (Self::TextBox, "text_box"),
        (Self::CheckBox, "check_box"),
        (Self::RadioButton, "radio_button"),
        (Self::ComboBox, "combo_box"),
        (Self::ListItem, "list_item"),
        (Self::MenuItem, "menu_item"),
        (Self::Tab, "tab"),
        (Self::TreeItem, "tree_item"),
        (Self::Table, "table"),
        (Self::Label, "label"),
        (Self::Image, "image"),
        (Self::Window, "window"),
        (Self::Other, "other"),
    ];

    /// The name used in files.
    pub fn as_str(self) -> &'static str {
        Self::ALL
            .iter()
            .find(|(c, _)| *c == self)
            .map_or("other", |(_, n)| n)
    }

    /// Parses a name used in files.
    pub fn parse(value: &str) -> Result<Self, PlanError> {
        Self::ALL
            .iter()
            .find(|(_, n)| *n == value)
            .map(|(c, _)| *c)
            .ok_or_else(|| PlanError::BadField {
                field: format!("class `{}`", clip(value)),
                msg: format!(
                    "expected one of {}",
                    Self::ALL
                        .iter()
                        .map(|(_, n)| *n)
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            })
    }
}

/// A relative query for one control, like Sighter's resolver: by text, name or class, inside a screen, near another target.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Target {
    /// Visible text.
    pub text: Option<String>,
    /// Accessible name.
    pub name: Option<String>,
    /// Control class.
    pub class: Option<ControlClass>,
    /// Restrict the search to this screen.
    pub inside: Option<ScreenName>,
    /// A neighbour that disambiguates this target.
    pub near: Option<Box<Target>>,
    /// Pick the n-th match (zero-based) when several remain.
    pub nth: Option<u16>,
}

impl Target {
    /// A target with no criteria matches nothing useful; the plan parser refuses it.
    pub fn is_empty(&self) -> bool {
        self.text.is_none() && self.name.is_none() && self.class.is_none() && self.near.is_none()
    }
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts: Vec<String> = Vec::new();
        if let Some(t) = &self.text {
            parts.push(format!("text={t:?}"));
        }
        if let Some(n) = &self.name {
            parts.push(format!("name={n:?}"));
        }
        if let Some(c) = self.class {
            parts.push(format!("class={}", c.as_str()));
        }
        if let Some(s) = &self.inside {
            parts.push(format!("inside={s}"));
        }
        if let Some(n) = &self.near {
            parts.push(format!("near=({n})"));
        }
        if let Some(n) = self.nth {
            parts.push(format!("nth={n}"));
        }
        f.write_str(&parts.join(" "))
    }
}

/// A step value: text, a column of the case's data table, or a secret by reference. Parsed once from `{{data.col}}` /
/// `{{secret:NAME}}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// Plain text (never contains `{{`).
    Literal(String),
    /// `{{data.COLUMN}}`.
    Data(Column),
    /// `{{secret:NAME}}`.
    Secret(SecretRef),
}

impl Value {
    /// Parses the file form.
    pub fn parse(text: &str) -> Result<Self, PlanError> {
        if !text.contains("{{") {
            return Ok(Self::Literal(text.to_owned()));
        }
        let inner = text
            .strip_prefix("{{")
            .and_then(|t| t.strip_suffix("}}"))
            .filter(|i| !i.contains("{{") && !i.contains("}}"));
        let bad = || PlanError::BadValue { value: clip(text) };
        let inner = inner.ok_or_else(bad)?;
        if let Some(col) = inner.strip_prefix("data.") {
            return Column::new(col).map(Self::Data);
        }
        if let Some(name) = inner.strip_prefix("secret:") {
            return SecretRef::new(name)
                .map(Self::Secret)
                .map_err(|_| PlanError::BadSecret {
                    value: clip(name),
                    msg: "not a valid secret name".to_owned(),
                });
        }
        Err(bad())
    }

    /// The file form, the inverse of [`Value::parse`].
    pub fn to_source(&self) -> String {
        match self {
            Self::Literal(t) => t.clone(),
            Self::Data(c) => format!("{{{{data.{c}}}}}"),
            Self::Secret(s) => format!("{{{{secret:{s}}}}}"),
        }
    }

    /// What the value looks like in reports: text, or the placeholder (never a resolved secret).
    pub fn describe(&self) -> String {
        format!("{:?}", self.to_source())
    }
}

/// One action. The set is closed: a plan cannot ask for anything else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Click a control.
    Click {
        /// The control.
        target: Target,
    },
    /// Type a value into a control.
    Type {
        /// The control.
        target: Target,
        /// What to type.
        value: Value,
    },
    /// Press a key combination, like `Ctrl+S`.
    Press {
        /// The keys.
        keys: String,
    },
    /// Choose an option in a list or combo box.
    Select {
        /// The control.
        target: Target,
        /// The option.
        option: Value,
    },
    /// Wait until a control exists.
    WaitFor {
        /// The control.
        target: Target,
        /// Longest wait.
        timeout_ms: u32,
    },
    /// Assert that a declared screen is showing.
    AssertScreen {
        /// The screen.
        expect_screen: ScreenName,
        /// Longest wait.
        timeout_ms: u32,
    },
    /// Assert that text is visible, optionally inside a control.
    AssertText {
        /// Where to look; the whole screen when `None`.
        target: Option<Target>,
        /// The text.
        text: Value,
    },
    /// Wait a fixed time.
    Wait {
        /// Milliseconds.
        ms: u32,
    },
}

impl Step {
    /// The action's name in files.
    pub fn action(&self) -> &'static str {
        match self {
            Self::Click { .. } => "click",
            Self::Type { .. } => "type",
            Self::Press { .. } => "press",
            Self::Select { .. } => "select",
            Self::WaitFor { .. } => "wait_for",
            Self::AssertScreen { .. } => "assert_screen",
            Self::AssertText { .. } => "assert_text",
            Self::Wait { .. } => "wait",
        }
    }

    /// Every target of the step.
    pub(crate) fn targets(&self) -> Vec<&Target> {
        match self {
            Self::Click { target }
            | Self::Type { target, .. }
            | Self::Select { target, .. }
            | Self::WaitFor { target, .. } => vec![target],
            Self::AssertText {
                target: Some(t), ..
            } => vec![t],
            _ => Vec::new(),
        }
    }

    /// Every value of the step.
    pub(crate) fn values(&self) -> Vec<&Value> {
        match self {
            Self::Type { value, .. } => vec![value],
            Self::Select { option, .. } => vec![option],
            Self::AssertText { text, .. } => vec![text],
            _ => Vec::new(),
        }
    }
}

/// The canonical one-line text of a step: what a plan diff compares and what reports show. A secret is its placeholder.
impl fmt::Display for Step {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.action())?;
        match self {
            Self::Click { target } => write!(f, " {target}"),
            Self::Type { target, value } => write!(f, " {target} value={}", value.describe()),
            Self::Press { keys } => write!(f, " {keys:?}"),
            Self::Select { target, option } => write!(f, " {target} option={}", option.describe()),
            Self::WaitFor { target, timeout_ms } => write!(f, " {target} timeout_ms={timeout_ms}"),
            Self::AssertScreen {
                expect_screen,
                timeout_ms,
            } => write!(f, " {expect_screen} timeout_ms={timeout_ms}"),
            Self::AssertText {
                target: Some(t),
                text,
            } => write!(f, " {t} text={}", text.describe()),
            Self::AssertText { target: None, text } => write!(f, " text={}", text.describe()),
            Self::Wait { ms } => write!(f, " ms={ms}"),
        }
    }
}
