//! The app's own login item on each OS, from one description of the app: `SMAppService.mainApp` on macOS
//! ([`super::MainAppItem`]), the per-user `Run` value on Windows ([`super::UserRunEntry`]) and an XDG autostart file on
//! Linux and the other Unix systems ([`super::XdgAutostart`]), each starting the app with its arguments (the hidden flag,
//! so it opens in the tray). Moved from Dyno's `login.rs` (`platform_autostart`); the app passes its executable, its
//! environment and the [`System`] in, so every branch is tested with fakes.

use super::{Autostart, UserRunEntry, XdgAutostart};
use crate::port::System;
use std::ffi::OsString;
use std::path::PathBuf;

/// The app's own login item, described once for every OS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppItem<'a> {
    /// The app as the desktop shows it (the XDG entry's `Name`).
    pub name: &'a str,
    /// The XDG file's name without `.desktop` (the file name an earlier route used, so an upgrade rewrites it).
    pub file_stem: &'a str,
    /// The value's name in `HKCU\...\Run` (Windows).
    pub run_value: &'a str,
    /// The arguments the login start passes (the hidden flag).
    pub args: &'a [&'a str],
}

/// A login item that cannot be made, with the reason: every change returns it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnavailableItem(pub String);

impl Autostart for UnavailableItem {
    fn set(&mut self, _on: bool) -> Result<(), String> {
        Err(self.0.clone())
    }
}

/// The folder XDG autostart files live under: `$XDG_CONFIG_HOME` when it is absolute (the XDG spec ignores a relative
/// one), otherwise `$HOME/.config`; `None` when neither is known.
pub fn xdg_config_dir(
    xdg_config_home: Option<OsString>,
    home: Option<OsString>,
) -> Option<PathBuf> {
    xdg_config_home
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| home.map(|h| PathBuf::from(h).join(".config")))
}

impl AppItem<'_> {
    /// The XDG autostart file `<config>/autostart/<file_stem>.desktop` that starts `exe` with the item's arguments.
    ///
    /// # Errors
    /// The config folder or the program is not known.
    pub fn xdg(
        &self,
        config: Option<PathBuf>,
        exe: std::io::Result<PathBuf>,
        sys: Box<dyn System>,
    ) -> Result<XdgAutostart, String> {
        match (config, exe) {
            (Some(config), Ok(exe)) => Ok(XdgAutostart::new(
                &config,
                self.file_stem,
                self.name,
                exe,
                self.args,
                sys,
            )),
            _ => Err("the home folder or the app's folder cannot be found".into()),
        }
    }

    /// The per-user `Run` value that starts `exe` with the item's arguments, never written while elevated.
    ///
    /// # Errors
    /// The program is not known.
    pub fn windows_run(
        &self,
        exe: std::io::Result<PathBuf>,
        sys: Box<dyn System>,
        elevation: Box<dyn super::Elevation>,
    ) -> Result<UserRunEntry, String> {
        let exe = exe.map_err(|e| format!("the app's folder cannot be found: {e}"))?;
        Ok(UserRunEntry::new(
            self.run_value,
            exe,
            self.args,
            sys,
            elevation,
        ))
    }

    /// The item this OS uses, for the program `exe` (the app passes `std::env::current_exe()`), reading the environment
    /// through `env` (the app passes `std::env::var_os`) and writing through `sys`. An item that cannot be made is an
    /// [`UnavailableItem`] that says why at the first change.
    pub fn for_this_os(
        &self,
        exe: std::io::Result<PathBuf>,
        env: &dyn Fn(&str) -> Option<OsString>,
        sys: Box<dyn System>,
    ) -> Box<dyn Autostart> {
        self.pick(exe, env, sys).into_autostart()
    }

    /// [`Self::for_this_os`] before it is boxed, so a test can tell which item was picked without touching the OS.
    #[cfg_attr(
        any(target_os = "macos", windows),
        allow(unused_variables, reason = "each OS uses only some of the inputs")
    )]
    fn pick(
        &self,
        exe: std::io::Result<PathBuf>,
        env: &dyn Fn(&str) -> Option<OsString>,
        sys: Box<dyn System>,
    ) -> Picked {
        #[cfg(target_os = "macos")]
        {
            Picked::MainApp(super::MainAppItem::new())
        }
        #[cfg(windows)]
        {
            match self.windows_run(exe, sys, Box::new(super::ProcessElevation)) {
                Ok(item) => Picked::Run(item),
                Err(why) => Picked::Unavailable(UnavailableItem(why)),
            }
        }
        #[cfg(not(any(target_os = "macos", windows)))]
        {
            match self.xdg(
                xdg_config_dir(env("XDG_CONFIG_HOME"), env("HOME")),
                exe,
                sys,
            ) {
                Ok(item) => Picked::Xdg(item),
                Err(why) => Picked::Unavailable(UnavailableItem(why)),
            }
        }
    }
}

/// The item [`AppItem::pick`] chose on this OS.
enum Picked {
    #[cfg(target_os = "macos")]
    MainApp(super::MainAppItem),
    #[cfg(windows)]
    Run(UserRunEntry),
    #[cfg(not(any(target_os = "macos", windows)))]
    Xdg(XdgAutostart),
    #[cfg_attr(
        target_os = "macos",
        allow(dead_code, reason = "macOS always has its main-app item")
    )]
    Unavailable(UnavailableItem),
}

impl Picked {
    fn into_autostart(self) -> Box<dyn Autostart> {
        match self {
            #[cfg(target_os = "macos")]
            Self::MainApp(item) => Box::new(item),
            #[cfg(windows)]
            Self::Run(item) => Box::new(item),
            #[cfg(not(any(target_os = "macos", windows)))]
            Self::Xdg(item) => Box::new(item),
            Self::Unavailable(item) => Box::new(item),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::login::Elevation;
    use crate::testing::FakeSystem;
    use std::ffi::OsString;
    #[cfg(not(any(target_os = "macos", windows)))]
    use std::path::Path;

    const DEMO: AppItem<'static> = AppItem {
        name: "Demo",
        file_stem: "Demo",
        run_value: "DemoRun",
        args: &["--hidden"],
    };

    struct Elev;
    impl Elevation for Elev {
        fn is_full(&mut self) -> bool {
            false
        }
    }

    /// An absolute path on every OS: `/x` on Unix, `C:/x` on Windows (a bare `/x` has no drive there, so is not absolute).
    fn abs(p: &str) -> PathBuf {
        if cfg!(windows) {
            PathBuf::from(format!("C:{p}"))
        } else {
            PathBuf::from(p)
        }
    }

    fn missing() -> std::io::Result<PathBuf> {
        Err(std::io::Error::new(std::io::ErrorKind::NotFound, "gone"))
    }

    #[test]
    fn the_config_folder_is_an_absolute_xdg_config_home_or_the_homes_dot_config() {
        let os = |s: &str| Some(abs(s).into_os_string());
        let rel = |s: &str| Some(OsString::from(s));
        assert_eq!(
            xdg_config_dir(os("/x/config"), os("/home/u")),
            Some(abs("/x/config"))
        );
        // A relative XDG_CONFIG_HOME is ignored (the XDG spec: such a value is invalid).
        assert_eq!(
            xdg_config_dir(rel("config"), os("/home/u")),
            Some(abs("/home/u").join(".config"))
        );
        assert_eq!(
            xdg_config_dir(None, os("/home/u")),
            Some(abs("/home/u").join(".config"))
        );
        assert_eq!(xdg_config_dir(rel("rel"), None), None);
        assert_eq!(xdg_config_dir(None, None), None);
    }

    #[test]
    fn on_linux_it_is_the_autostart_file_named_after_the_app_that_starts_it_hidden() {
        let sys = FakeSystem::new();
        let item = DEMO
            .xdg(
                Some(abs("/home/u").join(".config")),
                Ok(abs("/opt/demo/demo-app")),
                Box::new(sys.clone()),
            )
            .unwrap();
        assert_eq!(
            item.definition(),
            format!(
                "[Desktop Entry]\nType=Application\nName=Demo\nExec=\"{}\" \"--hidden\"\nTerminal=false\n\
                 X-GNOME-Autostart-enabled=true\n",
                abs("/opt/demo/demo-app").display()
            )
        );
        let mut item = item;
        item.set(true).unwrap();
        assert!(
            sys.file(
                &abs("/home/u/.config")
                    .join("autostart")
                    .join("Demo.desktop")
            )
            .is_some()
        );
    }

    #[test]
    fn on_linux_a_missing_home_or_program_is_said_in_words() {
        let sys = FakeSystem::new();
        let why = "the home folder or the app's folder cannot be found";
        assert_eq!(
            DEMO.xdg(None, Ok("/opt/demo/demo-app".into()), Box::new(sys.clone()))
                .err(),
            Some(why.to_string())
        );
        assert_eq!(
            DEMO.xdg(Some("/home/u/.config".into()), missing(), Box::new(sys))
                .err(),
            Some(why.to_string())
        );
    }

    #[test]
    fn on_windows_it_is_the_users_run_value_that_starts_it_hidden() {
        let sys = FakeSystem::new();
        let item = DEMO
            .windows_run(
                Ok(PathBuf::from(r"C:\Program Files\Demo\demo-app.exe")),
                Box::new(sys.clone()),
                Box::new(Elev),
            )
            .unwrap();
        assert_eq!(
            item.command(),
            r#""C:\Program Files\Demo\demo-app.exe" --hidden"#
        );
        let mut item = item;
        // The fake registry answers nothing, so the read-back fails; the write named the app's own value.
        assert!(item.set(true).is_err());
        assert!(
            sys.commands()
                .iter()
                .any(|c| c.starts_with("reg add HKCU") && c.contains("DemoRun"))
        );
    }

    #[test]
    fn on_windows_a_missing_program_is_said_in_words() {
        let err = DEMO
            .windows_run(missing(), Box::new(FakeSystem::new()), Box::new(Elev))
            .err()
            .unwrap();
        assert_eq!(err, "the app's folder cannot be found: gone");
    }

    #[test]
    fn an_item_that_cannot_be_made_refuses_every_change_with_its_reason() {
        let mut item = UnavailableItem("no home".into());
        assert_eq!(item.set(true), Err("no home".to_string()));
        assert_eq!(item.set(false), Err("no home".to_string()));
    }

    /// The item this OS uses, from the environment it is given (never the process's own).
    #[cfg(not(any(target_os = "macos", windows)))]
    #[test]
    fn this_os_uses_the_autostart_file_from_the_given_environment() {
        let sys = FakeSystem::new();
        let env = |k: &str| match k {
            "XDG_CONFIG_HOME" => Some(OsString::from("/x/cfg")),
            _ => None,
        };
        let mut item =
            DEMO.for_this_os(Ok("/opt/demo/demo-app".into()), &env, Box::new(sys.clone()));
        item.set(true).unwrap();
        assert!(
            sys.file(Path::new("/x/cfg/autostart/Demo.desktop"))
                .is_some()
        );

        let mut none = DEMO.for_this_os(Ok("/opt/demo/demo-app".into()), &|_| None, Box::new(sys));
        assert_eq!(
            none.set(true),
            Err("the home folder or the app's folder cannot be found".to_string())
        );
    }

    /// macOS picks `SMAppService.mainApp` whatever it is given (a missing program, no environment); nothing is registered.
    #[cfg(target_os = "macos")]
    #[test]
    fn macos_picks_the_main_app_item() {
        let picked = DEMO.pick(missing(), &|_| None, Box::new(FakeSystem::new()));
        assert!(matches!(picked, Picked::MainApp(_)));
        // Boxing keeps it (the trait object is what the app holds); set() is never called here (ER-14).
        drop(picked.into_autostart());
    }

    #[cfg(not(any(target_os = "macos", windows)))]
    #[test]
    fn elsewhere_the_pick_is_the_autostart_file_or_the_reason() {
        let env = |k: &str| (k == "HOME").then(|| OsString::from("/home/u"));
        let sys = || Box::new(FakeSystem::new());
        assert!(matches!(
            DEMO.pick(Ok("/opt/demo/demo-app".into()), &env, sys()),
            Picked::Xdg(_)
        ));
        assert!(matches!(
            DEMO.pick(missing(), &env, sys()),
            Picked::Unavailable(_)
        ));
    }

    #[cfg(windows)]
    #[test]
    fn this_os_uses_the_run_value_and_says_when_the_program_is_missing() {
        let mut item = DEMO.for_this_os(missing(), &|_| None, Box::new(FakeSystem::new()));
        assert_eq!(
            item.set(true),
            Err("the app's folder cannot be found: gone".to_string())
        );
    }
}
