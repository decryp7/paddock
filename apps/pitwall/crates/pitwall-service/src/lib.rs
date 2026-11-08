//! What Pitwall's background service is, declared once (rule 55, ADR 0056): the app registers it and the server runs as it, and
//! both build from this one [`spec`]. Every OS file is rendered from it (the bundled launch agent, the systemd unit, the
//! Windows service and the installer hooks), so the names, the display text and the kind cannot drift between the two programs.

use paddock_service::{Kind, ProcessType, ServiceSpec};

/// The launchd label; the bundled agent's plist is `<label>.plist` (`bundle.macOS.files` in `tauri.macos.conf.json`).
pub const LABEL: &str = "net.decryptology.pitwall.service";
/// What the service is called in messages and, on Windows, the name in the Service Control Manager.
pub const NAME: &str = "Pitwall";
/// The server program.
pub const PROGRAM: &str = "pitwall-server";
/// The variable that tells the server it is supervised (it adds the login shell's PATH and watches for updates).
pub const SUPERVISED_ENV: &str = "PITWALL_SERVICE";

/// Pitwall's service. On Windows it is a service of the Service Control Manager (the installer registers it once and the
/// app repeats that through Windows' own prompt); on macOS the bundled launch agent and on Linux a systemd user unit.
pub fn spec() -> ServiceSpec {
    spec_of(cfg!(windows))
}

/// [`spec`] for a Windows (`true`) or another target: the one place the kind depends on the OS, and what the tests render both of.
pub fn spec_of(windows: bool) -> ServiceSpec {
    let builder = ServiceSpec::builder(LABEL, NAME, PROGRAM)
        .unit("pitwall")
        .bundle_id("net.decryptology.pitwall")
        .display_name("Pitwall local LLM server")
        .description("Serves local language models to the apps on this PC (127.0.0.1 only).")
        .supervised_env(SUPERVISED_ENV)
        .process_type(ProcessType::Interactive)
        .docs_url("https://dev.decryptology.net/decryp7/pitwall");
    let builder = if windows { builder.kind(Kind::SystemService) } else { builder };
    builder.build().unwrap_or_else(|e| unreachable!("Pitwall's own spec is valid: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_service::render;

    #[test]
    fn the_spec_is_a_system_service_on_windows_and_an_agent_elsewhere() {
        assert_eq!(spec_of(true).kind(), Kind::SystemService);
        assert_eq!(spec_of(false).kind(), Kind::UserAgent);
        assert_eq!((spec().name(), spec().label(), spec().program()), (NAME, LABEL, PROGRAM));
    }

    #[test]
    fn the_windows_service_is_the_one_the_installer_and_the_server_expect() {
        let text = render::sc_script(&render::scm_config(&spec_of(true), r"C:\Program Files\Pitwall\pitwall-server.exe"));
        assert!(text.contains("create Pitwall") && text.contains("--service"), "{text}");
        assert!(text.contains("DisplayName= \"Pitwall local LLM server\""), "{text}");
        assert!(
            text.contains("restart/10000/restart/60000/restart/300000"),
            "the SCM restarts it: {text}"
        );
    }

    #[test]
    fn the_linux_unit_is_the_users_and_names_the_supervised_variable() {
        let unit = render::systemd_unit(&spec_of(false), "/home/ana/.local/share/pitwall/bin/pitwall-server", None);
        assert!(unit.contains("PITWALL_SERVICE=1"), "{unit}");
        assert!(unit.contains("ExecStart=") && unit.contains("pitwall-server"), "{unit}");
    }
}
