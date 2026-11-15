//! `fonts`: rebuilds the fonts embedded in Debrief's PDF reports (SIL OFL 1.1, see `OFL-*.txt`); the port of
//! `crates/debrief-pdf/fonts/build-fonts.sh`.
//!
//! Sources (the same families the UI ships via `@fontsource`): IBM Plex Sans 3.201 (variable) and JetBrains Mono 2.304. Needs
//! Python 3 with fontTools (`pip install fonttools==4.55.0`). Static instances are cut from the variable Plex fonts, then every font
//! is reduced to Latin, Greek, Cyrillic, punctuation, currency, letterlike symbols, arrows, maths operators, box drawing, geometric
//! shapes and U+FFFD, without hinting or OpenType layout tables (the PDF writer does no shaping).
//!
//! "Plex" is a Reserved Font Name under IBM's OFL, so the reduced (modified) Plex fonts are renamed "Debrief Report Sans" as OFL
//! condition 3 requires; JetBrains Mono has no reserved name and is renamed "Debrief Report Mono" only for consistency.

use crate::{run, Step};
use std::path::{Path, PathBuf};

/// The Unicode ranges kept.
pub const UNICODES: &str = "U+0020-007E,U+00A0-024F,U+0300-036F,U+0370-03FF,U+0400-052F,U+1E00-1EFF,U+2000-206F,U+20A0-20CF,U+2100-214F,U+2190-21FF,U+2200-22FF,U+2500-257F,U+25A0-25FF,U+2713,U+2717,U+FFFD";

/// One output font: the working file it is cut into, its file stem and PostScript name, its family and style.
pub struct Spec {
    pub work: &'static str,
    pub ps: &'static str,
    pub family: &'static str,
    pub style: &'static str,
}

/// The four fonts, in the order the script made them.
pub const SPECS: [Spec; 4] = [
    Spec {
        work: "regular",
        ps: "DebriefReportSans-Regular",
        family: "Debrief Report Sans",
        style: "Regular",
    },
    Spec {
        work: "bold",
        ps: "DebriefReportSans-Bold",
        family: "Debrief Report Sans",
        style: "Bold",
    },
    Spec {
        work: "italic",
        ps: "DebriefReportSans-Italic",
        family: "Debrief Report Sans",
        style: "Italic",
    },
    Spec {
        work: "mono",
        ps: "DebriefReportMono-Regular",
        family: "Debrief Report Mono",
        style: "Regular",
    },
];

/// The name-table rewrite of one subset font (fontTools): drops the old names and sets family, style, unique id, full name and
/// PostScript name. Arguments: source, destination, family, style, PostScript name.
pub const RENAME_PY: &str = r#"import sys
from fontTools.ttLib import TTFont
src, dst, family, style, ps = sys.argv[1:]
font = TTFont(src)
names = font["name"]
for rec in list(names.names):
    if rec.nameID in (1, 2, 3, 4, 6, 16, 17):
        names.removeNames(nameID=rec.nameID)
for nid, value in ((1, family), (2, style), (3, ps), (4, f"{family} {style}"), (6, ps)):
    names.setName(value, nid, 3, 1, 0x409)
font.save(dst)
"#;

/// Every step, in order: the three static instances, then per output font the subset and the rename.
pub fn plan(sans: &Path, sans_italic: &Path, tmp: &Path, out: &Path) -> Vec<Step> {
    let t = |n: &str| tmp.join(format!("{n}.ttf")).to_string_lossy().into_owned();
    let inst = |dst: &str, src: &Path, wght: &str| {
        Step::new(
            "python3",
            &[
                "-m",
                "fontTools.varLib.instancer",
                "-q",
                "-o",
                dst,
                &src.to_string_lossy(),
                &format!("wght={wght}"),
                "wdth=100",
            ],
        )
    };
    let mut steps = vec![
        inst(&t("regular"), sans, "400"),
        inst(&t("bold"), sans, "700"),
        inst(&t("italic"), sans_italic, "400"),
    ];
    for s in &SPECS {
        steps.push(Step::new(
            "python3",
            &[
                "-m",
                "fontTools.subset",
                &t(s.work),
                &format!("--unicodes={UNICODES}"),
                "--layout-features=",
                "--no-hinting",
                "--name-IDs=0,1,2,3,4,5,6,13,14",
                "--drop-tables+=GSUB,GPOS,GDEF,DSIG,STAT,MVAR,HVAR",
                &format!("--output-file={}", t(s.ps)),
            ],
        ));
        steps.push(Step::new(
            "python3",
            &[
                "-I",
                "-c",
                RENAME_PY,
                &t(s.ps),
                &out.join(format!("{}.ttf", s.ps)).to_string_lossy(),
                s.family,
                s.style,
                s.ps,
            ],
        ));
    }
    steps
}

/// `cargo xtask fonts <IBMPlexSans[wdth,wght].ttf> <IBMPlexSans-Italic[wdth,wght].ttf> <JetBrainsMono-Regular.ttf>`.
pub fn run_task(args: &[String], root: &Path) -> Result<(), String> {
    let [sans, italic, mono] = args else {
        return Err(
            "usage: cargo xtask fonts <IBMPlexSans[wdth,wght].ttf> <IBMPlexSans-Italic[wdth,wght].ttf> <JetBrainsMono-Regular.ttf>"
                .to_string(),
        );
    };
    for f in [sans, italic, mono] {
        if !Path::new(f).is_file() {
            return Err(format!(
                "{f} is not a file; download the sources listed in crates/debrief-pdf/src/fonts.rs"
            ));
        }
    }
    let out = root.join("crates/debrief-pdf/fonts");
    let base = ["TMPDIR", "TEMP", "TMP"]
        .iter()
        .find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
        .ok_or("no temp folder is set (TMPDIR); set it to a scratch folder")?;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    let tmp: PathBuf = Path::new(&base).join(format!("debrief-fonts-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&tmp).map_err(|e| format!("cannot create {}: {e}", tmp.display()))?;
    let result = (|| {
        std::fs::copy(mono, tmp.join("mono.ttf")).map_err(|e| format!("cannot copy {mono}: {e}"))?;
        for step in plan(Path::new(sans), Path::new(italic), &tmp, &out) {
            run(&step, root)?;
        }
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(&tmp);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_plan_is_three_instances_then_a_subset_and_a_rename_per_font() {
        let steps = plan(Path::new("/s.ttf"), Path::new("/i.ttf"), Path::new("/t"), Path::new("/o"));
        assert_eq!(steps.len(), 3 + 2 * SPECS.len());
        assert_eq!(
            steps[0].args.join(" "),
            "-m fontTools.varLib.instancer -q -o /t/regular.ttf /s.ttf wght=400 wdth=100"
        );
        assert_eq!(
            steps[1].args.join(" "),
            "-m fontTools.varLib.instancer -q -o /t/bold.ttf /s.ttf wght=700 wdth=100"
        );
        assert_eq!(
            steps[2].args.join(" "),
            "-m fontTools.varLib.instancer -q -o /t/italic.ttf /i.ttf wght=400 wdth=100"
        );
        let subset = steps[3].args.join(" ");
        assert!(
            subset.starts_with("-m fontTools.subset /t/regular.ttf --unicodes=U+0020-007E,"),
            "{subset}"
        );
        assert!(subset
            .contains("--layout-features= --no-hinting --name-IDs=0,1,2,3,4,5,6,13,14 --drop-tables+=GSUB,GPOS,GDEF,DSIG,STAT,MVAR,HVAR"));
        assert!(subset.ends_with("--output-file=/t/DebriefReportSans-Regular.ttf"));
        let rename = &steps[4].args;
        assert_eq!(&rename[..2], ["-I", "-c"]);
        assert_eq!(
            rename[3..],
            [
                "/t/DebriefReportSans-Regular.ttf",
                "/o/DebriefReportSans-Regular.ttf",
                "Debrief Report Sans",
                "Regular",
                "DebriefReportSans-Regular"
            ]
        );
        let last = steps.last().unwrap().args.join(" ");
        assert!(
            last.ends_with("/o/DebriefReportMono-Regular.ttf Debrief Report Mono Regular DebriefReportMono-Regular"),
            "{last}"
        );
    }

    #[test]
    fn the_ranges_are_the_scripts() {
        assert!(UNICODES.starts_with("U+0020-007E,U+00A0-024F") && UNICODES.ends_with("U+2713,U+2717,U+FFFD"));
        assert_eq!(UNICODES.split(',').count(), 16);
    }
}
