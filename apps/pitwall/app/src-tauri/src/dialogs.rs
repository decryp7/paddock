//! Pitwall's own dialog contracts (ADR 0026 §3, the PW-D row of `docs/plans/debrief-1-2-api-dialogs.md`). Each is a real
//! OS window (rule 17) that the shell opens fitted to its content. Pitwall's dialogs decide and return their choice; the
//! window that opened them acts on it (creates the app, quits, installs the service), so no dialog calls one of Pitwall's
//! commands itself and each contract lists only the start-up handshake every dialog window needs. The shared confirmation
//! (`pd.confirm`), About, What's new and Updates are the shell's tables. The slots panel stays an in-app popover (an
//! anchored panel, rule 17).

use paddock_shell::dialog_commands as shell;

paddock_dialogs::dialogs! {
    /// Pitwall's dialogs, and the commands every one of its dialog windows may call (the start-up handshake).
    pub static DIALOGS, base &[shell::startup::window_pong, shell::startup::window_ready];
    // "New app": the name (Create app is disabled until it is valid); the window that opened it creates the app.
    "pw.create-app" => Form(Modal) {
        width: Some(Narrow),
    },
    // The new key, shown once: Copy, then "I've saved it". No Cancel; Escape and the window's close do nothing.
    "pw.key-secret" => Decision {
        required_choice: true,
    },
    // "Quit Pitwall?": keep serving or stop serving, with "Don't ask again" in the body.
    "pw.quit" => Decision {
        width: Some(Medium),
    },
    // "Run Pitwall in the background?": Not now or Run in the background, with "Don't ask again" in the body; the install
    // then runs as a job in the main window.
    "pw.service-consent" => Decision {
        width: Some(Medium),
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The four contracts are valid, have the kinds and widths the plan gives them, and open nothing of Pitwall's to a
    /// dialog (the opener acts on the choice).
    #[test]
    fn the_contracts_are_the_planned_ones() {
        let c = paddock_dialogs::Contracts::from_tables(&[&DIALOGS]).unwrap();
        for (id, width) in [
            ("pw.create-app", 400.0),
            ("pw.key-secret", 400.0),
            ("pw.quit", 560.0),
            ("pw.service-consent", 560.0),
        ] {
            assert_eq!(c.get(id).map(|r| r.width), Some(width), "{id}");
            for command in ["create_app", "quit_app", "start_install_service", "save_settings"] {
                assert!(!c.allows(id, command), "{id} may not call {command}");
            }
        }
        let secret = DIALOGS.entries().iter().find(|e| e.id == "pw.key-secret").unwrap();
        assert!(
            secret.required_choice,
            "the key is shown once: the dialog cannot be dismissed without the answer"
        );
    }
}
