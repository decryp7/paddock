//! What a widget's button does (family ADR 0035 §5). The closed `WidgetActionId` becomes an [`Effect`] in dyno-core; this
//! module runs the effect through [`ActionPorts`], so the decision of what each button reaches is tested without a window or
//! an agent.

use crate::dto::CommandError;
use dyno_core::widgets::popover_values::{Effect, Navigate};

/// What the app can do for a button.
pub trait ActionPorts {
    /// Opens the main window at a page or a Settings section.
    fn navigate(&self, to: Navigate) -> Result<(), CommandError>;
    /// Asks for the popup's slow values again.
    fn refresh(&self);
    /// Restarts the agent through the app's own supervisor, never through the agent.
    fn restart_agent(&self);
}

/// Runs `effect`.
pub fn run(effect: Effect, ports: &dyn ActionPorts) -> Result<(), CommandError> {
    match effect {
        Effect::Navigate(to) => ports.navigate(to),
        Effect::Refresh => {
            ports.refresh();
            Ok(())
        }
        Effect::RestartAgent => {
            ports.restart_agent();
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_core::widgets::popover_values::{Page, WidgetActionId};
    use paddock_widgets::PlacementId;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Recorder(Mutex<Vec<String>>);

    impl Recorder {
        fn log(&self, what: &str) {
            self.0.lock().unwrap().push(what.into());
        }
        fn calls(&self) -> Vec<String> {
            self.0.lock().unwrap().clone()
        }
    }

    impl ActionPorts for Recorder {
        fn navigate(&self, to: Navigate) -> Result<(), CommandError> {
            self.log(&format!("navigate {to:?}"));
            Ok(())
        }
        fn refresh(&self) {
            self.log("refresh");
        }
        fn restart_agent(&self) {
            self.log("restart");
        }
    }

    fn run_action(a: WidgetActionId) -> Vec<String> {
        let r = Recorder::default();
        run(a.effect(), &r).unwrap();
        r.calls()
    }

    #[test]
    fn each_button_reaches_exactly_one_port() {
        assert_eq!(run_action(WidgetActionId::OpenPage(Page::Alarms)), ["navigate Page(Alarms)"]);
        assert_eq!(run_action(WidgetActionId::CustomizeWidgets), ["navigate Settings(PopupWidgets)"]);
        assert_eq!(
            run_action(WidgetActionId::Retry(PlacementId::new("disk-space").unwrap())),
            ["refresh"]
        );
        assert_eq!(run_action(WidgetActionId::RestartAgent), ["restart"]);
    }
}
