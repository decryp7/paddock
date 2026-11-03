//! The installed-software inventory and the inventory report. The agent reads the OS's own lists (macOS
//! `system_profiler SPApplicationsDataType -json`, Windows `reg query ...\Uninstall /s`, Linux `dpkg-query`/`rpm`/`pacman`);
//! these functions parse them and render the report. The report is written only to a path the user picked.

use super::{clean, clip};
use crate::inventory::SystemInventory;
use crate::time::Timestamp;
use crate::units::bytes;
use serde::{Deserialize, Serialize};

/// Most items kept (a developer machine has thousands of packages; more than this is cut and says so).
pub const MAX_ITEMS: usize = 5_000;

/// One installed program or package.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SoftwareItem {
    /// Name.
    pub name: String,
    /// Version, when the OS says.
    pub version: Option<String>,
    /// Publisher or source, when the OS says.
    pub publisher: Option<String>,
}

/// The software list, or why it is unavailable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SoftwareInventory {
    /// Items by name.
    pub items: Vec<SoftwareItem>,
    /// More items exist than were kept.
    pub truncated: bool,
    /// Why the list could not be read.
    pub unavailable: Option<String>,
    /// When the list was last read, when this answer is the saved one of an earlier run (shown at once while the agent reads
    /// again); `None` when it was read just now (ADR 0029).
    #[serde(default)]
    pub saved_at: Option<crate::time::Timestamp>,
}

impl SoftwareInventory {
    /// A list from parsed items: sorted by name, deduplicated, capped.
    pub fn from_items(mut items: Vec<SoftwareItem>) -> SoftwareInventory {
        items.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then(a.version.cmp(&b.version)));
        items.dedup();
        let truncated = items.len() > MAX_ITEMS;
        items.truncate(MAX_ITEMS);
        SoftwareInventory {
            items,
            truncated,
            unavailable: None,
            saved_at: None,
        }
    }

    /// An empty list with the reason.
    pub fn unavailable(why: impl Into<String>) -> SoftwareInventory {
        SoftwareInventory {
            unavailable: Some(why.into()),
            ..Default::default()
        }
    }
}

fn item(name: &str, version: Option<&str>, publisher: Option<&str>) -> Option<SoftwareItem> {
    let name = clip(name.trim(), 200);
    if name.is_empty() {
        return None;
    }
    let opt = |v: Option<&str>| v.map(str::trim).filter(|v| !v.is_empty()).map(|v| clip(v, 100));
    Some(SoftwareItem {
        name,
        version: opt(version),
        publisher: opt(publisher),
    })
}

/// `system_profiler SPApplicationsDataType -json`.
pub fn parse_system_profiler(text: &str) -> Vec<SoftwareItem> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else {
        return Vec::new();
    };
    v.get("SPApplicationsDataType")
        .and_then(|a| a.as_array())
        .map(|apps| {
            apps.iter()
                .filter_map(|a| {
                    let s = |k: &str| a.get(k).and_then(|x| x.as_str());
                    item(s("_name")?, s("version"), s("obtained_from"))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// `reg query HKLM\Software\Microsoft\Windows\CurrentVersion\Uninstall /s`: a key line, then
/// `    DisplayName    REG_SZ    Name` lines (also `DisplayVersion`, `Publisher`). Entries without a display name
/// (components, hotfix stubs) are skipped, and so are entries flagged `SystemComponent`.
pub fn parse_reg_uninstall(text: &str) -> Vec<SoftwareItem> {
    let mut out = Vec::new();
    let (mut name, mut version, mut publisher, mut system) = (None::<String>, None::<String>, None::<String>, false);
    let mut flush = |name: &mut Option<String>, version: &mut Option<String>, publisher: &mut Option<String>, system: &mut bool| {
        if let Some(n) = name.take() {
            if !*system {
                out.extend(item(&n, version.as_deref(), publisher.as_deref()));
            }
        }
        *version = None;
        *publisher = None;
        *system = false;
    };
    for raw in text.lines() {
        let line = clean(raw);
        if line.starts_with("HKEY_") {
            flush(&mut name, &mut version, &mut publisher, &mut system);
            continue;
        }
        let mut parts = line.splitn(3, "    ");
        let (Some(key), Some(_kind), Some(value)) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        match key.trim() {
            "DisplayName" => name = Some(value.trim().to_string()),
            "DisplayVersion" => version = Some(value.trim().to_string()),
            "Publisher" => publisher = Some(value.trim().to_string()),
            "SystemComponent" => system = value.trim() == "0x1",
            _ => {}
        }
    }
    flush(&mut name, &mut version, &mut publisher, &mut system);
    out
}

/// `name<TAB>version` per line (`dpkg-query -W -f='${Package}\t${Version}\n'`, `rpm -qa --qf '%{NAME}\t%{VERSION}-%{RELEASE}\n'`)
/// or `name version` (`pacman -Q`).
pub fn parse_package_list(text: &str) -> Vec<SoftwareItem> {
    text.lines()
        .map(clean)
        .filter_map(|l| {
            let (n, v) = l.split_once('\t').or_else(|| l.split_once(' '))?;
            item(n, Some(v), None)
        })
        .collect()
}

/// The format of the inventory report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InventoryFormat {
    /// A readable Markdown file.
    Markdown,
    /// JSON for other tools.
    Json,
}

impl InventoryFormat {
    /// The file extension.
    pub fn extension(self) -> &'static str {
        match self {
            InventoryFormat::Markdown => "md",
            InventoryFormat::Json => "json",
        }
    }
}

/// Checks where the report may be written: an absolute path ending in the format's extension, without `..`.
pub fn validate_destination(path: &str, format: InventoryFormat) -> Result<std::path::PathBuf, String> {
    if path.trim().is_empty() || path.len() > 1024 || path.contains('\0') {
        return Err("Choose where to save the report.".into());
    }
    let p = std::path::Path::new(path);
    if !p.is_absolute() {
        return Err("Give the full path of the file to save.".into());
    }
    if crate::disk::has_dot_segment(p) {
        return Err("The save path may not contain \"..\".".into());
    }
    let ext = format.extension();
    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("").to_ascii_lowercase();
    if !name.ends_with(&format!(".{ext}")) || name.len() <= ext.len() + 1 {
        return Err(format!("Save the report as a file ending in .{ext}."));
    }
    Ok(p.to_path_buf())
}

/// What the report holds.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonReport<'a> {
    generated_at: Timestamp,
    hardware: &'a SystemInventory,
    software: &'a SoftwareInventory,
}

/// Renders the report. Local text only: no serial numbers or user names are included (the hardware inventory holds
/// none), and nothing is sent anywhere.
pub fn render_report(format: InventoryFormat, at: Timestamp, hardware: &SystemInventory, software: &SoftwareInventory) -> String {
    match format {
        InventoryFormat::Json => serde_json::to_string_pretty(&JsonReport {
            generated_at: at,
            hardware,
            software,
        })
        .unwrap_or_default(),
        InventoryFormat::Markdown => markdown(at, hardware, software),
    }
}

fn markdown(at: Timestamp, hw: &SystemInventory, sw: &SoftwareInventory) -> String {
    let mut s = String::new();
    s.push_str("# Inventory report\n\n");
    s.push_str(&format!(
        "Generated by Dyno at {} (Unix ms). Local file; nothing was sent anywhere.\n\n",
        at.millis()
    ));
    s.push_str("## Hardware\n\n");
    s.push_str(&format!("- System: {}\n", cell(&hw.os)));
    if let Some(h) = &hw.host_name {
        s.push_str(&format!("- Computer name: {}\n", cell(h)));
    }
    s.push_str(&format!(
        "- Processor: {} ({} cores, {} threads{})\n",
        cell(&hw.cpu.name),
        hw.cpu.physical_cores,
        hw.cpu.logical_cores,
        if hw.cpu.frequency_mhz > 0 {
            format!(", {} MHz", hw.cpu.frequency_mhz)
        } else {
            String::new()
        }
    ));
    s.push_str(&format!("- Memory: {}\n", bytes(hw.memory.total())));
    for d in &hw.disks {
        s.push_str(&format!(
            "- Disk: {}, {}{}\n",
            cell(&d.name),
            bytes(d.total_bytes),
            if d.removable { " (removable)" } else { "" }
        ));
    }
    s.push_str("\n## Software\n\n");
    match &sw.unavailable {
        Some(why) => s.push_str(&format!("The software list could not be read: {}\n", cell(why))),
        None => {
            s.push_str("| Name | Version | Publisher |\n| --- | --- | --- |\n");
            for i in &sw.items {
                s.push_str(&format!(
                    "| {} | {} | {} |\n",
                    cell(&i.name),
                    cell(i.version.as_deref().unwrap_or("")),
                    cell(i.publisher.as_deref().unwrap_or(""))
                ));
            }
            if sw.truncated {
                s.push_str(&format!("\nOnly the first {MAX_ITEMS} items are listed.\n"));
            }
        }
    }
    s
}

/// Text safe inside a Markdown table cell: no pipes, no line breaks, no markup that renders as HTML.
fn cell(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '|' => '/',
            '<' | '>' | '\n' | '\r' | '\t' => ' ',
            c => c,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::{CpuInfo, DiskInfo, MemoryKind, MemoryLayout};

    #[test]
    fn system_profiler_apps() {
        let j = r#"{"SPApplicationsDataType":[
          {"_name":"Safari","version":"26.0","obtained_from":"apple","path":"/Applications/Safari.app"},
          {"_name":"Code","version":"1.99","obtained_from":"identified_developer"},
          {"version":"1"}, {"_name":" "}]}"#;
        let items = parse_system_profiler(j);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].name, "Safari");
        assert_eq!(items[0].publisher.as_deref(), Some("apple"));
        assert!(parse_system_profiler("not json").is_empty());
        assert!(parse_system_profiler("{}").is_empty());
    }

    #[test]
    fn windows_uninstall_keys() {
        let t = "
HKEY_LOCAL_MACHINE\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{AAA}
    DisplayName    REG_SZ    Example Editor
    DisplayVersion    REG_SZ    4.2.1
    Publisher    REG_SZ    Example Ltd

HKEY_LOCAL_MACHINE\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{BBB}
    DisplayName    REG_SZ    Hidden Runtime
    SystemComponent    REG_DWORD    0x1

HKEY_LOCAL_MACHINE\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{CCC}
    UninstallString    REG_SZ    x.exe
";
        let items = parse_reg_uninstall(t);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].name, "Example Editor");
        assert_eq!(items[0].version.as_deref(), Some("4.2.1"));
    }

    #[test]
    fn package_lists() {
        let items = parse_package_list("bash\t5.2\ncurl 8.4\n\nbroken\n");
        assert_eq!(items.len(), 2);
        assert_eq!(items[1].version.as_deref(), Some("8.4"));
    }

    #[test]
    fn inventories_are_sorted_deduplicated_and_capped() {
        let mk = |n: &str| SoftwareItem {
            name: n.into(),
            version: None,
            publisher: None,
        };
        let inv = SoftwareInventory::from_items(vec![mk("b"), mk("A"), mk("b")]);
        assert_eq!(inv.items.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(), ["A", "b"]);
        let many: Vec<_> = (0..MAX_ITEMS + 3).map(|i| mk(&format!("n{i:05}"))).collect();
        let inv = SoftwareInventory::from_items(many);
        assert!(inv.truncated);
        assert_eq!(inv.items.len(), MAX_ITEMS);
        assert_eq!(SoftwareInventory::unavailable("no tool").unavailable.as_deref(), Some("no tool"));
    }

    fn hardware() -> SystemInventory {
        SystemInventory::new(
            "macOS 26".into(),
            Some("studio".into()),
            CpuInfo {
                name: "Apple M4".into(),
                physical_cores: 10,
                logical_cores: 10,
                frequency_mhz: 0,
            },
            MemoryLayout::unified(16 * 1024 * 1024 * 1024, MemoryKind::default(), None),
            vec![DiskInfo {
                name: "Macintosh HD".into(),
                total_bytes: 1_000_000_000_000,
                removable: false,
            }],
        )
    }

    #[test]
    fn the_destination_must_be_a_plain_absolute_file_with_the_right_extension() {
        // Absolute on every platform: a bare "/tmp" has no drive on Windows.
        let t = if cfg!(windows) { "C:\\tmp" } else { "/tmp" };
        assert!(validate_destination(&format!("{t}/r.md"), InventoryFormat::Markdown).is_ok());
        assert!(validate_destination(&format!("{t}/R.JSON"), InventoryFormat::Json).is_ok());
        for bad in [
            "".to_string(),
            "r.md".to_string(),
            format!("{t}/r.json"),
            format!("{t}/.md"),
            format!("{t}/../r.md"),
            format!("{t}/a\0.md"),
        ] {
            assert!(validate_destination(&bad, InventoryFormat::Markdown).is_err(), "{bad}");
        }
        assert!(validate_destination(&format!("{t}/{}.md", "a".repeat(2_000)), InventoryFormat::Markdown).is_err());
    }

    #[test]
    fn the_report_renders_in_both_formats_and_escapes_cells() {
        let sw = SoftwareInventory::from_items(vec![SoftwareItem {
            name: "A|B <b>x</b>".into(),
            version: Some("1".into()),
            publisher: None,
        }]);
        let md = render_report(InventoryFormat::Markdown, Timestamp(5), &hardware(), &sw);
        assert!(md.contains("# Inventory report") && md.contains("Apple M4") && md.contains("Macintosh HD"));
        assert!(md.contains("| A/B  b x /b  | 1 |  |"));
        assert!(!md.contains("<b>"));
        let js = render_report(InventoryFormat::Json, Timestamp(5), &hardware(), &sw);
        let v: serde_json::Value = serde_json::from_str(&js).unwrap();
        assert_eq!(v["generatedAt"], 5);
        assert_eq!(v["software"]["items"][0]["version"], "1");
        let none = SoftwareInventory::unavailable("system_profiler is not available");
        assert!(render_report(InventoryFormat::Markdown, Timestamp(5), &hardware(), &none).contains("could not be read"));
        assert_eq!(InventoryFormat::Markdown.extension(), "md");
        assert_eq!(InventoryFormat::Json.extension(), "json");
    }
}
