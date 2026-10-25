//! Optional actions when a watch finds a new release: open its location, download it to a folder (with checksum
//! verification and resume), or run a program the user chose with the file as an argument. Commands never go through
//! a shell: the program and each argument are separate, and placeholders are replaced inside one argument only.

use crate::limits::MAX_COMMAND_ARGS;
use crate::release::Release;
use serde::{Deserialize, Serialize};

/// One action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Action {
    /// Open the release's location (folder, file or page) with the system's default handler. With a release that has
    /// several files, `variant` and `extension` choose which one (empty: the primary file).
    OpenLocation {
        /// The variant to open (`x64`).
        #[serde(default)]
        variant: String,
        /// The extension to open (`iso`).
        #[serde(default)]
        extension: String,
    },
    /// Copy or download the release into a folder.
    Download {
        /// The destination folder.
        folder: String,
        /// Verify the publisher's SHA-256 (or a `.sha256` file next to it) and refuse the file when it differs.
        #[serde(default = "yes")]
        verify: bool,
        /// The variant to download (`x64`); empty for the primary file.
        #[serde(default)]
        variant: String,
        /// The extension to download (`iso`); empty for the primary file.
        #[serde(default)]
        extension: String,
    },
    /// Run a program (opt-in; the user picked it). `{path}` is the downloaded file (or the location when nothing was
    /// downloaded), `{version}`, `{name}` and `{location}` the release's.
    RunCommand {
        /// Full path of the program.
        program: String,
        /// Its arguments.
        #[serde(default)]
        args: Vec<String>,
    },
}

fn yes() -> bool {
    true
}

impl Action {
    /// Opens the primary file of a release.
    pub fn open_location() -> Action {
        Action::OpenLocation {
            variant: String::new(),
            extension: String::new(),
        }
    }

    /// Downloads the primary file of a release.
    pub fn download(folder: &str, verify: bool) -> Action {
        Action::Download {
            folder: folder.into(),
            verify,
            variant: String::new(),
            extension: String::new(),
        }
    }

    /// Checks an action; the message says what to fix.
    pub fn check(&self) -> Result<(), String> {
        match self {
            Action::OpenLocation { .. } => Ok(()),
            Action::Download { folder, .. } if folder.trim().is_empty() => Err("Choose a folder to download into.".into()),
            Action::Download { .. } => Ok(()),
            Action::RunCommand { program, .. } if !is_absolute(program) => {
                Err("Choose the program to run with its full path (Spotter does not search PATH).".into())
            }
            Action::RunCommand { args, .. } if args.len() > MAX_COMMAND_ARGS => {
                Err(format!("A command can have at most {MAX_COMMAND_ARGS} arguments."))
            }
            Action::RunCommand { args, .. } if args.iter().any(|a| a.len() > 4096 || a.contains('\0')) => {
                Err("An argument is too long or contains a NUL character.".into())
            }
            Action::RunCommand { .. } => Ok(()),
        }
    }
}

fn is_absolute(p: &str) -> bool {
    let b = p.as_bytes();
    p.starts_with('/')
        || p.starts_with(r"\\")
        || (b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/'))
}

/// The arguments of a command for a release, placeholders replaced (each argument stays one argument).
pub fn command_args(args: &[String], release: &Release, path: &str) -> Vec<String> {
    args.iter()
        .map(|a| {
            a.replace("{path}", path)
                .replace("{version}", &release.version.text)
                .replace("{name}", &release.name)
                .replace("{location}", &release.location)
        })
        .collect()
}

/// A safe file name for a downloaded release: the last segment of its location (without query), with characters
/// that are not allowed in file names replaced, reserved Windows names prefixed, never empty, at most 200 bytes.
pub fn file_name(release: &Release) -> String {
    let loc = release.location.split(['?', '#']).next().unwrap_or("");
    let last = loc.rsplit(['/', '\\']).find(|s| !s.is_empty()).unwrap_or("");
    // An image reference (`app:1.2.0`) or a bare scheme is not a file name.
    let last = if last.contains(':') { "" } else { last };
    let decoded = url::form_urlencoded::parse(format!("x={}", last.replace('+', "%2B")).as_bytes())
        .next()
        .map(|(_, v)| v.into_owned())
        .unwrap_or_default();
    let mut name: String = decoded
        .chars()
        .map(|c| if c.is_control() || "<>:\"/\\|?*".contains(c) { '_' } else { c })
        .collect();
    name = name.trim_matches([' ', '.']).to_string();
    if name.is_empty() {
        name = format!("release-{}", release.version.text)
            .chars()
            .map(|c| if "<>:\"/\\|?* ".contains(c) { '_' } else { c })
            .collect();
    }
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = ["CON", "PRN", "AUX", "NUL"].contains(&stem.as_str())
        || ((stem.starts_with("COM") || stem.starts_with("LPT")) && stem.len() == 4 && stem.as_bytes()[3].is_ascii_digit());
    if reserved {
        name = format!("_{name}");
    }
    while name.len() > 200 {
        name.pop();
    }
    name
}

/// Where a download resumes: the bytes already in the part file, unless they cannot belong to the file (more than
/// its known size), in which case it starts over.
pub fn resume_offset(part_len: u64, total: Option<u64>) -> u64 {
    match total {
        Some(t) if part_len > t => 0,
        _ => part_len,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::version::Version;

    fn rel(location: &str) -> Release {
        Release {
            version: Version::new("1.2.0", vec![1, 2, 0], ""),
            name: "App 1.2".into(),
            location: location.into(),
            published_at: None,
            size: None,
            sha256: None,
            detail: Default::default(),
        }
    }

    #[test]
    fn actions_check_their_settings() {
        assert!(Action::open_location().check().is_ok());
        assert!(Action::OpenLocation {
            variant: "x64".into(),
            extension: "iso".into()
        }
        .check()
        .is_ok());
        assert!(Action::download(" ", true).check().is_err());
        assert!(Action::download("D:\\x", false).check().is_ok());
        let run = |p: &str, args: Vec<String>| Action::RunCommand { program: p.into(), args }.check();
        assert!(run("C:\\Tools\\install.exe", vec![]).is_ok());
        assert!(run("/usr/local/bin/x", vec![]).is_ok());
        assert!(run(r"\\server\tools\x.exe", vec![]).is_ok());
        assert!(run("install.exe", vec![]).unwrap_err().contains("full path"));
        assert!(run("C:x", vec![]).is_err());
        assert!(run("/x", vec!["a".into(); MAX_COMMAND_ARGS + 1]).is_err());
        assert!(run("/x", vec!["a\0b".into()]).is_err());
        assert!(run("/x", vec!["a".repeat(5000)]).is_err());
        let toml_text = "kind = \"download\"\nfolder = \"/d\"";
        assert_eq!(toml::from_str::<Action>(toml_text).unwrap(), Action::download("/d", true));
    }

    #[test]
    fn placeholders_stay_inside_their_argument() {
        let args = vec![
            "/i".into(),
            "{path}".into(),
            "--tag={version}".into(),
            "{name}; rm -rf /".into(),
            "{location}".into(),
        ];
        let out = command_args(&args, &rel("https://dl/x.msi"), "C:\\Down loads\\x.msi");
        assert_eq!(
            out,
            [
                "/i",
                "C:\\Down loads\\x.msi",
                "--tag=1.2.0",
                "App 1.2; rm -rf /",
                "https://dl/x.msi"
            ]
        );
    }

    #[test]
    fn file_names_are_safe() {
        assert_eq!(file_name(&rel("https://dl/a/app%201.2.zip?token=x")), "app 1.2.zip");
        assert_eq!(file_name(&rel(r"\\server\share\R1.01.00T10.txt")), "R1.01.00T10.txt");
        assert_eq!(file_name(&rel("https://dl/a/..")), "release-1.2.0");
        assert_eq!(file_name(&rel("ghcr.io/o/app:1.2.0")), "release-1.2.0");
        assert_eq!(file_name(&rel("https://dl/a%3Cb%3E.zip")), "a_b_.zip");
        assert_eq!(file_name(&rel("https://dl/CON.txt")), "_CON.txt");
        assert_eq!(file_name(&rel("https://dl/com1")), "_com1");
        assert_eq!(file_name(&rel("https://dl/COMX.txt")), "COMX.txt");
        assert_eq!(file_name(&rel(&format!("https://dl/{}", "é".repeat(150)))).len(), 200);
        assert_eq!(file_name(&rel("")), "release-1.2.0");
    }

    #[test]
    fn resume_offsets() {
        assert_eq!(resume_offset(10, Some(100)), 10);
        assert_eq!(resume_offset(10, None), 10);
        assert_eq!(resume_offset(200, Some(100)), 0);
    }
}
