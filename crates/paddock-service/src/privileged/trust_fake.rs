//! A fake machine for [`TrustPort`] and the default Windows security descriptors it is built from (`C:\`, `C:\ProgramData`,
//! `C:\Program Files` as `icacls` describes a stock install; the VM check confirms them on a real machine). For tests of this
//! crate and of the apps that use the rule; no real data.

use super::files::FileKind;
use super::trust::*;
use std::collections::HashMap;
use std::io;

/// A fake machine: nodes by path (Windows paths compare lower-case).
pub struct FakePort {
    pub style: Style,
    pub nodes: HashMap<String, Node>,
    pub links: HashMap<String, String>,
    pub calls: Vec<String>,
}

impl FakePort {
    fn sep(&self) -> &'static str {
        if self.style == Style::Windows {
            "\\"
        } else {
            "/"
        }
    }
    pub fn key(&self, p: &str) -> String {
        match self.style {
            Style::Windows => p.replace('/', "\\").trim_end_matches('\\').to_lowercase(),
            Style::Unix => p.trim_end_matches('/').to_string(),
        }
    }
    pub fn put(&mut self, p: &str, n: Node) -> &mut Self {
        let k = self.key(p);
        self.nodes.insert(k, n);
        self
    }
}

impl TrustPort for FakePort {
    fn style(&self) -> Style {
        self.style
    }
    fn node(&mut self, path: &str) -> io::Result<Option<Node>> {
        Ok(self.nodes.get(&self.key(path)).cloned())
    }
    fn models_identity(&self) -> bool {
        false
    }
    fn aside_name_parts(&self) -> usize {
        1
    }
    fn canonical(&mut self, path: &str) -> io::Result<String> {
        Ok(self
            .links
            .get(path)
            .cloned()
            .unwrap_or_else(|| path.to_string()))
    }
    fn walk(&mut self, path: &str, limit: usize) -> io::Result<Vec<(String, Node)>> {
        let key = self.key(path);
        let sep = self.sep();
        let mut out: Vec<(String, Node)> = self
            .nodes
            .iter()
            .filter(|(k, _)| k.starts_with(&format!("{key}{sep}")))
            .map(|(k, n)| (k.clone(), n.clone()))
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        if out.len() > limit {
            return Err(io::Error::other("too many entries"));
        }
        Ok(out)
    }
    fn create_dir(&mut self, path: &str) -> io::Result<()> {
        self.calls.push(format!("create_dir {path}"));
        let key = self.key(path);
        if self.nodes.contains_key(&key) {
            return Err(io::Error::from(io::ErrorKind::AlreadyExists));
        }
        let made = match self.style {
            Style::Windows => locked(),
            Style::Unix => unix_dir(0, 0o755),
        };
        self.nodes.insert(key, made);
        Ok(())
    }
    fn lock_dir(&mut self, path: &str) -> io::Result<()> {
        self.calls.push(format!("lock_dir {path}"));
        let key = self.key(path);
        let made = match self.style {
            Style::Windows => locked(),
            Style::Unix => unix_dir(0, 0o755),
        };
        match self.nodes.get_mut(&key) {
            Some(n) => {
                *n = made;
                Ok(())
            }
            None => Err(io::Error::from(io::ErrorKind::NotFound)),
        }
    }
    fn rename_aside(&mut self, path: &str) -> io::Result<String> {
        self.calls.push(format!("rename_aside {path}"));
        let key = self.key(path);
        let sep = self.sep();
        let taken = self
            .nodes
            .keys()
            .filter(|k| {
                k.starts_with(&format!("{key}.untrusted-")) && !k[key.len() + 11..].contains(sep)
            })
            .count();
        let aside = format!("{path}.untrusted-{}", taken + 1);
        let aside_key = self.key(&aside);
        let moved: Vec<String> = self
            .nodes
            .keys()
            .filter(|k| **k == key || k.starts_with(&format!("{key}{sep}")))
            .cloned()
            .collect();
        for k in moved {
            if let Some(n) = self.nodes.remove(&k) {
                self.nodes
                    .insert(format!("{aside_key}{}", &k[key.len()..]), n);
            }
        }
        Ok(aside)
    }
    fn sweep_aside(&mut self, parent: &str, name: &str, keep: usize) -> io::Result<Vec<String>> {
        self.calls
            .push(format!("sweep_aside {parent} {name} {keep}"));
        let pkey = self.key(parent);
        let sep = self.sep();
        let prefix = format!("{pkey}{sep}{}.untrusted-", name.to_lowercase());
        let mut asides: Vec<(u64, String)> = self
            .nodes
            .iter()
            .filter_map(|(k, node)| {
                let rest = k.strip_prefix(&prefix)?;
                let n: u64 = rest.parse().ok()?;
                let trusted = match &node.security {
                    Security::Windows(d) => is_trusted_sid(&d.owner),
                    Security::Unix { owner, .. } => *owner == 0,
                };
                (node.kind == FileKind::Dir && trusted).then(|| (n, k.clone()))
            })
            .collect();
        asides.sort_by_key(|a| std::cmp::Reverse(a.0));
        let mut removed = Vec::new();
        for (_, key) in asides.into_iter().skip(keep) {
            let tree = format!("{key}{sep}");
            self.nodes.retain(|k, _| *k != key && !k.starts_with(&tree));
            removed.push(key);
        }
        Ok(removed)
    }
    fn remove_tree(&mut self, path: &str) -> io::Result<()> {
        self.calls.push(format!("remove_tree {path}"));
        let key = self.key(path);
        let sep = self.sep();
        self.nodes
            .retain(|k, _| k != &key && !k.starts_with(&format!("{key}{sep}")));
        Ok(())
    }
}

pub const RX: u32 = 0x0012_00a9;
pub const FULL: u32 = 0x001f_01ff;
pub const MODIFY: u32 = 0x0013_01bf;
/// WD, AD, WEA: what stock `C:\ProgramData` gives Users.
pub const WD_AD_WEA: u32 = 0x0000_0016;
/// AD alone: what stock `C:\` gives Authenticated Users.
pub const AD: u32 = 0x0000_0004;

pub fn win(owner: &str, dacl: Vec<Ace>) -> Node {
    Node {
        kind: FileKind::Dir,
        security: Security::Windows(Descriptor {
            owner: owner.into(),
            dacl: Some(dacl),
        }),
    }
}

/// `C:\` as captured on a Windows VM (owner TrustedInstaller).
pub const SDDL_DRIVE_ROOT: &str = "O:S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464G:S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464D:PAI(A;;LC;;;AU)(A;OICIIO;SDGXGWGR;;;AU)(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;0x1200a9;;;BU)(A;;0x1000a1;;;S-1-15-3-65536-1888954469-739942743-1668119174-2468466756-4239452838-1296943325-355587736-700089176)";
/// `C:\ProgramData` (owner SYSTEM): Users `(A;CI;DCLCRPCR;;;BU)` may add files and folders.
pub const SDDL_PROGRAM_DATA: &str = "O:SYG:SYD:PAI(A;OICIIO;GA;;;CO)(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;0x1200a9;;;BU)(A;CI;DCLCRPCR;;;BU)";
/// `C:\Program Files` (owner TrustedInstaller).
pub const SDDL_PROGRAM_FILES: &str = "O:S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464G:S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464D:PAI(A;OICIIO;GA;;;CO)(A;OICIIO;GA;;;SY)(A;;0x1301bf;;;SY)(A;OICIIO;GA;;;BA)(A;;0x1301bf;;;BA)(A;OICIIO;GXGR;;;BU)(A;;0x1200a9;;;BU)(A;CIIO;GA;;;S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464)(A;;FA;;;S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464)(A;;0x1200a9;;;AC)(A;OICIIO;GXGR;;;AC)(A;;0x1200a9;;;S-1-15-2-2)(A;OICIIO;GXGR;;;S-1-15-2-2)";
/// `C:\Program Files\Dyno Helper`, the locked folder the installer makes (owner Administrators).
pub const SDDL_PROGRAM_FILES_DYNO: &str =
    "O:BAG:S-1-5-21-1-2-513D:PAI(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;0x1200a9;;;BU)";
/// `C:\ProgramData\Dyno\Helper` as the unfixed helper left it: everything inherited from ProgramData, including Users
/// `(A;CIID;DCLCRPCR;;;BU)`, which let a standard user create files and folders in it.
pub const SDDL_DYNO_HELPER_UNFIXED: &str = "O:BAG:S-1-5-21-1-2-513D:AI(A;OICIID;FA;;;SY)(A;OICIID;FA;;;BA)(A;OICIIOID;GA;;;CO)(A;OICIID;0x1200a9;;;BU)(A;CIID;DCLCRPCR;;;BU)";

fn alias(s: &str) -> String {
    match s {
        "SY" => SID_SYSTEM,
        "BA" => SID_ADMINISTRATORS,
        "BU" => SID_USERS,
        "AU" => SID_AUTHENTICATED_USERS,
        "CO" => SID_CREATOR_OWNER,
        "WD" => SID_EVERYONE,
        "AC" => "S-1-15-2-1",
        other => other,
    }
    .to_string()
}

fn rights_mask(s: &str) -> u32 {
    if let Some(hex) = s.strip_prefix("0x") {
        return u32::from_str_radix(hex, 16).expect("hex mask");
    }
    let mut mask = 0;
    let b = s.as_bytes();
    for pair in b.chunks(2) {
        mask |= match std::str::from_utf8(pair).expect("ascii") {
            "FA" => 0x001f_01ff,
            "FR" => 0x0012_0089,
            "FW" => 0x0012_0116,
            "FX" => 0x0012_00a0,
            "GA" => 0x1000_0000,
            "GR" => 0x8000_0000,
            "GW" => 0x4000_0000,
            "GX" => 0x2000_0000,
            "CC" => 0x1,
            "DC" => 0x2,
            "LC" => 0x4,
            "SW" => 0x8,
            "RP" => 0x10,
            "WP" => 0x20,
            "DT" => 0x40,
            "LO" => 0x80,
            "CR" => 0x100,
            "SD" => 0x1_0000,
            "RC" => 0x2_0000,
            "WD" => 0x4_0000,
            "WO" => 0x8_0000,
            other => panic!("rights word {other}"),
        };
    }
    mask
}

/// A node from an SDDL string (`O:` owner and `D:` entries; allow and deny entries; flags `IO` and `ID` read).
pub fn from_sddl(kind: FileKind, sddl: &str) -> Node {
    let owner = sddl
        .strip_prefix("O:")
        .and_then(|r| r.split("G:").next())
        .map(alias)
        .expect("owner");
    let dacl = sddl.split_once("D:").expect("dacl").1;
    let aces = dacl
        .split('(')
        .skip(1)
        .map(|a| {
            let f: Vec<&str> = a.trim_end_matches(')').split(';').collect();
            Ace {
                sid: alias(f[5]),
                allow: f[0] == "A",
                mask: rights_mask(f[2]),
                inherit_only: f[1].contains("IO"),
            }
        })
        .collect();
    Node {
        kind,
        security: Security::Windows(Descriptor {
            owner,
            dacl: Some(aces),
        }),
    }
}

/// Stock `C:\`: Users read and execute, Authenticated Users list; what they create they may modify (inherit-only).
pub fn stock_drive_root() -> Node {
    from_sddl(FileKind::Dir, SDDL_DRIVE_ROOT)
}

/// Stock `C:\ProgramData`: Users may create files and folders in it.
pub fn stock_program_data() -> Node {
    from_sddl(FileKind::Dir, SDDL_PROGRAM_DATA)
}

/// A folder the unfixed helper made in `C:\ProgramData`: everything inherited, Users may create things in it.
pub fn inherited_from_program_data() -> Node {
    from_sddl(FileKind::Dir, SDDL_DYNO_HELPER_UNFIXED)
}

/// A Unix folder.
pub fn unix_dir(owner: u32, mode: u32) -> Node {
    Node {
        kind: FileKind::Dir,
        security: Security::Unix {
            owner,
            mode,
            write_acl: false,
        },
    }
}

/// A folder after `protect`: SYSTEM and Administrators full, Users read and execute, owner Administrators.
pub fn locked() -> Node {
    win(
        SID_ADMINISTRATORS,
        vec![
            Ace::allow(SID_SYSTEM, FULL),
            Ace::allow(SID_ADMINISTRATORS, FULL),
            Ace::allow(SID_USERS, RX),
        ],
    )
}

pub fn file(mut n: Node) -> Node {
    n.kind = FileKind::File;
    n
}

pub const SMARTCTL: &str =
    "C:\\ProgramData\\Dyno\\Helper\\tools\\smartmontools-7.5\\bin\\smartctl.exe";
pub const TOOLS: &str = "C:\\ProgramData\\Dyno\\Helper\\tools";

/// A Windows machine with stock `C:\` and `C:\ProgramData` and the helper's folders given by `below`.
pub fn windows_machine(below: impl Fn(&str) -> Node) -> FakePort {
    let mut p = FakePort {
        style: Style::Windows,
        nodes: HashMap::new(),
        links: HashMap::new(),
        calls: vec![],
    };
    p.put("C:\\", stock_drive_root())
        .put("C:\\ProgramData", stock_program_data());
    for dir in [
        "C:\\ProgramData\\Dyno",
        "C:\\ProgramData\\Dyno\\Helper",
        TOOLS,
        "C:\\ProgramData\\Dyno\\Helper\\tools\\smartmontools-7.5",
        "C:\\ProgramData\\Dyno\\Helper\\tools\\smartmontools-7.5\\bin",
    ] {
        p.put(dir, below(dir));
    }
    p.put(SMARTCTL, file(below(SMARTCTL)));
    p
}
