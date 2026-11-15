//! `./dev diagrams`: the draw.io export and the generators of an app's diagrams (docs/plans/shell-to-rust.md, WP-7; rule 55: one
//! implementation, replacing the five identical `export-diagrams.sh` copies and Dyno's `export.sh`, `build.sh` and Debrief's
//! `generate.sh`).
//!
//! `export` writes, for each `<src>/<name>.drawio`, `<out>/svg/<name>.svg` (transparent, theme-aware, system fonts, the PNG copy
//! of every HTML label dropped) and `<out>/png/<name>.png` (2x, light, opaque). The draw.io flags are [`SVG_ARGS`] and
//! [`PNG_ARGS`]; `scripts/svg-drift.mjs` keeps a copy of the SVG list until it moves here (docs/known-copies.toml). Exports run one at a
//! time, as the script's did, to keep the machine quiet.
//!
//! `generate` runs the generators an app lists in `docs/diagrams/src/generators.txt` (one file name per line, `#` comments) with
//! `node`, in order, from that folder, and with `--export` exports the result.

use crate::ci_tool::{Output, TAIL_LINES, ToolCall, error_line, tail};
use clap::{Args, Subcommand};
use paddock_cli::{CliError, Report};
use regex::Regex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::Duration;

/// The draw.io flags of an SVG export, before `--output <file> <source> --no-sandbox`.
pub const SVG_ARGS: &[&str] = &[
    "--export",
    "--format",
    "svg",
    "--transparent",
    "--embed-svg-fonts",
    "false",
    "--border",
    "16",
];
/// The draw.io flags of a PNG export, before `--output <file> <source> --no-sandbox`.
pub const PNG_ARGS: &[&str] = &["--export", "--format", "png", "--scale", "2", "--border", "16", "--theme", "light"];

/// One draw.io export, or one generator, at most this long (draw.io is an Electron app; a hang must not hold a job).
const STEP_TIMEOUT: Duration = Duration::from_secs(600);

/// The display, session and profile variables draw.io (Electron) needs on a Linux or macOS desktop besides `PATH` and `HOME`.
const DRAWIO_ENV: &[&str] = &[
    "DISPLAY",
    "XAUTHORITY",
    "WAYLAND_DISPLAY",
    "XDG_*",
    "DBUS_SESSION_BUS_ADDRESS",
    "LC_*",
    "ELECTRON_*",
];

/// `./dev diagrams <command>`.
#[derive(Subcommand)]
pub enum DiagramsCommand {
    /// Export every `.drawio` of a folder (or the named ones) to `svg/` and `png/` with the draw.io desktop CLI.
    Export(ExportArgs),
    /// Run the app's diagram generators (docs/diagrams/src/generators.txt) with node, in order.
    Generate(GenerateArgs),
}

/// The arguments of `diagrams export`.
#[derive(Args)]
pub struct ExportArgs {
    /// An app under `apps/`: relative folders are read against `apps/<app>`. Without it, against the current folder.
    #[arg(long)]
    app: Option<String>,
    /// The folder with the `.drawio` sources.
    #[arg(long, default_value = "docs/diagrams")]
    src: PathBuf,
    /// Where `svg/` and `png/` go (default: the source folder).
    #[arg(long)]
    out: Option<PathBuf>,
    /// The draw.io executable (default: `$DRAWIO`, then the usual places).
    #[arg(long)]
    drawio: Option<PathBuf>,
    /// The repository root (default: the current repository).
    #[arg(long)]
    root: Option<PathBuf>,
    /// Diagram names without `.drawio` (default: every `.drawio` in the source folder, in name order).
    names: Vec<String>,
}

/// The arguments of `diagrams generate`.
#[derive(Args)]
pub struct GenerateArgs {
    /// The app under `apps/` whose generators run.
    #[arg(long)]
    app: String,
    /// Export the diagrams afterwards (the names, if any, limit the export; every generator still runs).
    #[arg(long)]
    export: bool,
    /// With `--export`: the folder with the `.drawio` sources, relative to the app (Dyno keeps them in `diagrams`).
    #[arg(long, default_value = "docs/diagrams")]
    src: PathBuf,
    /// The draw.io executable for `--export`.
    #[arg(long)]
    drawio: Option<PathBuf>,
    /// The repository root (default: the current repository).
    #[arg(long)]
    root: Option<PathBuf>,
    /// With `--export`: diagram names to export.
    names: Vec<String>,
}

/// Runs one `diagrams` command.
pub fn command(command: DiagramsCommand) -> Result<Report, CliError> {
    match command {
        DiagramsCommand::Export(a) => export(&a),
        DiagramsCommand::Generate(a) => generate(&a),
    }
}

fn env_map() -> HashMap<String, String> {
    std::env::vars().collect()
}

/// The folder the relative paths of a command are read against: `apps/<app>` of the repository, or the current folder.
fn base_dir(app: Option<&str>, root: Option<&Path>) -> Result<PathBuf, CliError> {
    match app {
        Some(app) => {
            if app.is_empty() || app.contains(['/', '\\']) || app == "." || app == ".." {
                return Err(CliError::usage(format!("`--app {app}` is not an app folder name.")));
            }
            let root = crate::gates::command::repo_root(root)?;
            let dir = root.join("apps").join(app);
            if dir.is_dir() {
                Ok(dir)
            } else {
                Err(CliError::usage(format!("There is no app folder {}.", dir.display()))
                    .with_hint("Name an app under apps/, for example `--app pitcrew`."))
            }
        }
        None => std::env::current_dir().map_err(|e| CliError::failure(format!("The current folder cannot be read: {e}."))),
    }
}

/// The draw.io executable: the given path, `$DRAWIO`, then `drawio` and `draw.io` on `PATH` and the vendor's install places.
pub fn find_drawio(given: Option<&Path>, env: &HashMap<String, String>, windows: bool) -> Option<PathBuf> {
    if let Some(p) = given {
        return Some(p.to_path_buf());
    }
    if let Some(p) = env.get("DRAWIO").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(p));
    }
    let path = env.get("PATH").or_else(|| env.get("Path")).map_or("", String::as_str);
    let ext = crate::pathfind::extensions(env.get("PATHEXT").map(String::as_str));
    for name in ["drawio", "draw.io"] {
        if let Some(found) = crate::pathfind::find_program(path, name, windows, &ext) {
            return Some(found);
        }
    }
    let mut places: Vec<PathBuf> = vec![PathBuf::from("/Applications/draw.io.app/Contents/MacOS/draw.io")];
    if windows {
        for var in ["ProgramFiles", "ProgramW6432"] {
            if let Some(dir) = env.get(var) {
                places.push(Path::new(dir).join("draw.io").join("draw.io.exe"));
            }
        }
        places.push(PathBuf::from(r"C:\Program Files\draw.io\draw.io.exe"));
    }
    places.into_iter().find(|p| p.is_file())
}

static LABEL_COPY: LazyLock<Regex> = LazyLock::new(|| {
    // The patterns are literals of this file, checked by the tests below. Same expression as the script's JavaScript.
    #[allow(clippy::expect_used)]
    let re = Regex::new(r#"(</foreignObject>)<image [^>]*xlink:href="data:image/png;base64,[^"]*"[^>]*/>"#).expect("a fixed regex");
    re
});

/// draw.io adds a PNG copy of every HTML label (inside `<switch>`, after its `<foreignObject>`) for viewers without foreignObject
/// support. Browsers, Gitea and GitHub render the HTML, so the copies are dropped: they bloat each file.
pub fn strip_label_copies(svg: &str) -> String {
    LABEL_COPY.replace_all(svg, "$1").into_owned()
}

/// The names of the `.drawio` files of `src`, sorted.
fn all_names(src: &Path) -> Result<Vec<String>, CliError> {
    let entries = std::fs::read_dir(src).map_err(|e| {
        CliError::failure(error_line(
            &format!("The diagram folder {} cannot be read: {e}.", src.display()),
            "check `--src` (relative to the app with `--app`).",
        ))
    })?;
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .filter_map(|e| {
            let p = e.path();
            (p.extension().is_some_and(|x| x == "drawio") && p.is_file())
                .then(|| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
                .flatten()
        })
        .collect();
    names.sort();
    Ok(names)
}

fn drawio_failure(program: &Path, what: &str, how: &str, stderr: &str) -> CliError {
    CliError::failure(format!(
        "{}\n{}",
        error_line(
            &format!("draw.io ({}) failed on {what}: {how}.", program.display()),
            "run it by hand on that file; on Linux a display is needed (xvfb-run), and `--drawio`/`DRAWIO` names another executable."
        ),
        tail(stderr, TAIL_LINES).trim_end()
    ))
}

fn run_drawio(drawio: &Path, flags: &[&str], target: &Path, source: &Path, env: &HashMap<String, String>) -> Result<(), CliError> {
    let call = ToolCall::new(drawio.to_string_lossy())
        .args(flags.iter().copied())
        .args([
            "--output".to_string(),
            target.to_string_lossy().into_owned(),
            source.to_string_lossy().into_owned(),
            "--no-sandbox".to_string(),
        ])
        .pass(DRAWIO_ENV.iter().copied())
        .timeout(STEP_TIMEOUT);
    let ran = call
        .run(env, &mut std::io::stderr())
        .map_err(|why| CliError::failure(error_line(&why, "install draw.io desktop or set DRAWIO to its executable.")))?;
    if ran.success {
        Ok(())
    } else {
        Err(drawio_failure(drawio, &source.display().to_string(), &ran.how, &ran.stderr))
    }
}

/// The exports `args` ask for, as the lines the script printed (`exported <name>`).
pub fn export(args: &ExportArgs) -> Result<Report, CliError> {
    let base = base_dir(args.app.as_deref(), args.root.as_deref())?;
    let src = base.join(&args.src);
    let out = base.join(args.out.as_ref().unwrap_or(&args.src));
    let env = env_map();
    let drawio = find_drawio(args.drawio.as_deref(), &env, cfg!(windows)).ok_or_else(|| {
        CliError::failure(error_line(
            "draw.io desktop was not found.",
            "install it (or use the CI image), or set DRAWIO or pass --drawio.",
        ))
    })?;
    let names = if args.names.is_empty() {
        all_names(&src)?
    } else {
        args.names.clone()
    };
    // A missing source is checked for every name before the first export, so a typo costs nothing.
    for n in &names {
        let f = src.join(format!("{n}.drawio"));
        if !f.is_file() {
            return Err(CliError::failure(error_line(
                &format!("Missing {}.", f.display()),
                "check the diagram name and `--src`.",
            )));
        }
    }
    let (svg_dir, png_dir) = (out.join("svg"), out.join("png"));
    for d in [&svg_dir, &png_dir] {
        std::fs::create_dir_all(d).map_err(|e| {
            CliError::failure(error_line(
                &format!("{} cannot be created: {e}.", d.display()),
                "check the folder's rights.",
            ))
        })?;
    }
    let mut lines = Vec::new();
    for n in &names {
        let source = src.join(format!("{n}.drawio"));
        let svg = svg_dir.join(format!("{n}.svg"));
        run_drawio(&drawio, SVG_ARGS, &svg, &source, &env)?;
        let text = std::fs::read_to_string(&svg).map_err(|e| {
            CliError::failure(error_line(
                &format!("{} was not written by draw.io: {e}.", svg.display()),
                "run draw.io by hand on the source.",
            ))
        })?;
        std::fs::write(&svg, strip_label_copies(&text)).map_err(|e| {
            CliError::failure(error_line(
                &format!("{} cannot be written: {e}.", svg.display()),
                "check the folder's rights.",
            ))
        })?;
        run_drawio(&drawio, PNG_ARGS, &png_dir.join(format!("{n}.png")), &source, &env)?;
        lines.push(format!("exported {n}"));
    }
    Ok(Report::new(
        serde_json::json!({ "exported": names, "svg": svg_dir.display().to_string(), "png": png_dir.display().to_string() }),
        lines.join("\n"),
    ))
}

/// The generator names of a `generators.txt` text, in order: blank lines and `#` comments skipped, a name without a path.
pub fn parse_generators(text: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.contains(['/', '\\']) || !line.ends_with(".mjs") {
            return Err(format!(
                "line {}: `{line}` is not a generator file name (a plain `<name>.mjs` in the same folder).",
                i + 1
            ));
        }
        out.push(line.to_string());
    }
    Ok(out)
}

/// Runs the generators of an app, then the export with `--export`.
pub fn generate(args: &GenerateArgs) -> Result<Report, CliError> {
    let base = base_dir(Some(&args.app), args.root.as_deref())?;
    let dir = base.join("docs").join("diagrams").join("src");
    let list = dir.join("generators.txt");
    let text = std::fs::read_to_string(&list).map_err(|e| {
        CliError::failure(error_line(
            &format!("{} cannot be read: {e}.", list.display()),
            "list the app's generator files there, one `<name>.mjs` per line, in the order they run.",
        ))
    })?;
    let generators =
        parse_generators(&text).map_err(|why| CliError::failure(error_line(&format!("{}: {why}", list.display()), "fix the line.")))?;
    let env = env_map();
    for g in &generators {
        let ran = ToolCall::new("node")
            .args([g.as_str()])
            .cwd(&dir)
            .output(Output::Stream)
            .timeout(STEP_TIMEOUT)
            .run(&env, &mut std::io::stderr())
            .map_err(|why| CliError::failure(error_line(&why, "install node 20 or newer.")))?;
        if !ran.success {
            return Err(CliError::failure(error_line(
                &format!("generator {g} failed: {}.", ran.how),
                "read its output above, fix it and run `./dev diagrams generate` again.",
            )));
        }
    }
    let generated = Report::new(
        serde_json::json!({ "generated": generators }),
        format!("generated {} diagram files", generators.len()),
    );
    if !args.export {
        return Ok(generated);
    }
    let exported = export(&ExportArgs {
        app: Some(args.app.clone()),
        src: args.src.clone(),
        out: None,
        drawio: args.drawio.clone(),
        root: args.root.clone(),
        names: args.names.clone(),
    })?;
    Ok(Report::new(
        serde_json::json!({ "generated": generators, "export": exported.data }),
        format!("{}\n{}", generated.text, exported.text),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_label_copies_are_dropped_and_the_rest_is_kept() {
        let svg = r#"<switch><foreignObject>a</foreignObject><image x="1" xlink:href="data:image/png;base64,AAAA=="
 width="3"/></switch><image xlink:href="data:image/png;base64,BBBB" width="2"/><foreignObject>b</foreignObject>"#;
        assert_eq!(
            strip_label_copies(svg),
            r#"<switch><foreignObject>a</foreignObject></switch><image xlink:href="data:image/png;base64,BBBB" width="2"/><foreignObject>b</foreignObject>"#
        );
    }

    #[test]
    fn generators_skip_comments_and_refuse_paths() {
        assert_eq!(parse_generators("# c\n\n c4.mjs \nseq.mjs\n").unwrap(), ["c4.mjs", "seq.mjs"]);
        assert!(parse_generators("../x.mjs").is_err());
        assert!(parse_generators("run.sh").is_err());
    }

    #[test]
    fn the_given_executable_wins_over_the_variable_and_the_path() {
        let env: HashMap<String, String> = [("DRAWIO".to_string(), "/env/drawio".to_string())].into();
        assert_eq!(find_drawio(Some(Path::new("/given")), &env, false), Some(PathBuf::from("/given")));
        assert_eq!(find_drawio(None, &env, false), Some(PathBuf::from("/env/drawio")));
    }
}
