//! Hardware inventory: what the machine is made of that rarely changes. Today the memory layout (slots, sticks,
//! speeds) with plain-language upgrade hints; read once at agent start and on demand, never in the sampling loop.
//! Pure: the OS adapters parse what the OS reports into these types (ADR 0001).

use serde::{Deserialize, Serialize};

/// Most modules one array may hold (guards against absurd input).
pub const MAX_MODULES: usize = 64;

/// The generation of a memory module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum MemoryKind {
    /// DDR2.
    Ddr2,
    /// DDR3.
    Ddr3,
    /// DDR4.
    Ddr4,
    /// DDR5.
    Ddr5,
    /// LPDDR3.
    Lpddr3,
    /// LPDDR4 (including 4X).
    Lpddr4,
    /// LPDDR5 (including 5X).
    Lpddr5,
    /// Some other type.
    Other,
    /// The OS did not say.
    #[default]
    Unknown,
}

impl MemoryKind {
    /// From an SMBIOS memory type code (`Win32_PhysicalMemory.SMBIOSMemoryType`).
    pub fn from_smbios(code: u32) -> MemoryKind {
        match code {
            19 => MemoryKind::Ddr2,
            24 => MemoryKind::Ddr3,
            26 => MemoryKind::Ddr4,
            27 => MemoryKind::Lpddr3,
            28 => MemoryKind::Lpddr4,
            34 => MemoryKind::Ddr5,
            35 => MemoryKind::Lpddr5,
            0 | 2 => MemoryKind::Unknown,
            _ => MemoryKind::Other,
        }
    }

    /// From a name such as "DDR4", "LPDDR5X" or "LPDDR4" (case-insensitive).
    pub fn from_name(name: &str) -> MemoryKind {
        let n = name.trim().to_ascii_uppercase();
        if n.is_empty() {
            return MemoryKind::Unknown;
        }
        match () {
            _ if n.starts_with("LPDDR5") => MemoryKind::Lpddr5,
            _ if n.starts_with("LPDDR4") => MemoryKind::Lpddr4,
            _ if n.starts_with("LPDDR3") => MemoryKind::Lpddr3,
            _ if n.starts_with("DDR5") => MemoryKind::Ddr5,
            _ if n.starts_with("DDR4") => MemoryKind::Ddr4,
            _ if n.starts_with("DDR3") => MemoryKind::Ddr3,
            _ if n.starts_with("DDR2") => MemoryKind::Ddr2,
            _ => MemoryKind::Other,
        }
    }

    /// A short label, e.g. "DDR4".
    pub fn label(self) -> &'static str {
        match self {
            MemoryKind::Ddr2 => "DDR2",
            MemoryKind::Ddr3 => "DDR3",
            MemoryKind::Ddr4 => "DDR4",
            MemoryKind::Ddr5 => "DDR5",
            MemoryKind::Lpddr3 => "LPDDR3",
            MemoryKind::Lpddr4 => "LPDDR4",
            MemoryKind::Lpddr5 => "LPDDR5",
            MemoryKind::Other => "Memory",
            MemoryKind::Unknown => "Memory",
        }
    }
}

/// The physical shape of a module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum FormFactor {
    /// Desktop stick.
    Dimm,
    /// Laptop stick.
    SoDimm,
    /// Soldered to the board.
    Soldered,
    /// Something else.
    Other,
    /// The OS did not say.
    #[default]
    Unknown,
}

impl FormFactor {
    /// From an SMBIOS form factor code (`Win32_PhysicalMemory.FormFactor`).
    pub fn from_smbios(code: u32) -> FormFactor {
        match code {
            8 => FormFactor::Dimm,
            12 => FormFactor::SoDimm,
            11 | 14 | 15 => FormFactor::Soldered,
            0 | 2 => FormFactor::Unknown,
            _ => FormFactor::Other,
        }
    }
}

/// One memory module (a stick, or a soldered package).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryModule {
    /// The slot's label as the firmware names it, e.g. "ChannelA-DIMM0".
    pub slot: String,
    /// The bank label, when the firmware gives one.
    pub bank: Option<String>,
    /// Size in bytes.
    pub capacity_bytes: u64,
    /// The module's rated speed in MT/s.
    pub speed_mts: Option<u32>,
    /// The speed it actually runs at in MT/s.
    pub configured_speed_mts: Option<u32>,
    /// Generation.
    pub kind: MemoryKind,
    /// Shape.
    pub form_factor: FormFactor,
    /// Manufacturer, when known.
    pub manufacturer: Option<String>,
    /// Part number, when known.
    pub part_number: Option<String>,
}

impl MemoryModule {
    /// The speed it runs at: the configured one, else the rated one.
    pub fn running_mts(&self) -> Option<u32> {
        self.configured_speed_mts.or(self.speed_mts)
    }
}

/// The memory slots of a machine and what sits in them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MemoryArray {
    /// How many slots the board has (0 when unknown).
    pub slots: u32,
    /// The largest total the board supports, in bytes, when known.
    pub max_capacity_bytes: Option<u64>,
    /// The populated slots.
    pub modules: Vec<MemoryModule>,
}

impl MemoryArray {
    /// The installed total in bytes.
    pub fn installed_bytes(&self) -> u64 {
        self.modules.iter().fold(0u64, |a, m| a.saturating_add(m.capacity_bytes))
    }

    /// Slots without a module (0 when the slot count is unknown).
    pub fn free_slots(&self) -> u32 {
        self.slots.saturating_sub(self.modules.len().min(u32::MAX as usize) as u32)
    }

    /// Caps the module list so hostile input stays bounded.
    pub fn bounded(mut self) -> MemoryArray {
        self.modules.truncate(MAX_MODULES);
        self.slots = self.slots.min(MAX_MODULES as u32 * 4);
        self
    }
}

/// Why the memory detail is not available.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Unavailable {
    /// A sentence for the user.
    pub reason: String,
}

impl Unavailable {
    /// Linux: the DMI tables are root-only.
    pub fn linux_dmi_root_only() -> Unavailable {
        Unavailable {
            reason: "Linux only lets an administrator read the memory slot table. Dyno shows the total only.".into(),
        }
    }

    /// Reading failed for another reason.
    pub fn failed(reason: impl Into<String>) -> Unavailable {
        Unavailable { reason: reason.into() }
    }
}

/// What is known about the machine's memory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum MemoryLayout {
    /// Slots and sticks.
    Slots(MemoryArray),
    /// Memory in the processor package (Apple silicon): nothing to upgrade.
    Unified {
        /// Total in bytes.
        total: u64,
        /// Generation.
        kind: MemoryKind,
        /// Manufacturer, when known.
        manufacturer: Option<String>,
        /// Always false: it cannot be upgraded.
        upgradeable: bool,
    },
    /// Only the total is known.
    TotalOnly {
        /// Total in bytes.
        total: u64,
        /// Why more is not known.
        reason: Unavailable,
    },
}

impl MemoryLayout {
    /// Unified memory (not upgradeable).
    pub fn unified(total: u64, kind: MemoryKind, manufacturer: Option<String>) -> MemoryLayout {
        MemoryLayout::Unified {
            total,
            kind,
            manufacturer,
            upgradeable: false,
        }
    }

    /// Total bytes.
    pub fn total(&self) -> u64 {
        match self {
            MemoryLayout::Slots(a) => a.installed_bytes(),
            MemoryLayout::Unified { total, .. } | MemoryLayout::TotalOnly { total, .. } => *total,
        }
    }

    /// Upgrade hints for the layout (empty unless it has slots).
    pub fn hints(&self) -> Vec<Hint> {
        match self {
            MemoryLayout::Slots(a) => memory_hints(a),
            MemoryLayout::Unified { .. } => vec![Hint::new(
                HintKind::NotUpgradeable,
                "This memory is part of the processor package and cannot be upgraded.",
                None,
            )],
            MemoryLayout::TotalOnly { reason, .. } => vec![Hint::new(HintKind::Unknown, reason.reason.clone(), None)],
        }
    }
}

/// Basic CPU facts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CpuInfo {
    /// Marketing name.
    pub name: String,
    /// Physical cores (0 when unknown).
    pub physical_cores: u32,
    /// Logical processors.
    pub logical_cores: u32,
    /// Nominal frequency in MHz (0 when unknown).
    pub frequency_mhz: u32,
}

/// One disk in the inventory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskInfo {
    /// Name.
    pub name: String,
    /// Size in bytes.
    pub total_bytes: u64,
    /// An external or removable disk.
    pub removable: bool,
}

/// The machine's inventory, for the Hardware page and the report export.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemInventory {
    /// OS name and version.
    pub os: String,
    /// The host name.
    pub host_name: Option<String>,
    /// CPU.
    pub cpu: CpuInfo,
    /// Memory.
    pub memory: MemoryLayout,
    /// Disks.
    pub disks: Vec<DiskInfo>,
    /// Upgrade hints derived from the memory layout.
    pub hints: Vec<Hint>,
}

impl SystemInventory {
    /// Assembles the inventory and derives the hints.
    pub fn new(os: String, host_name: Option<String>, cpu: CpuInfo, memory: MemoryLayout, disks: Vec<DiskInfo>) -> SystemInventory {
        let hints = memory.hints();
        SystemInventory {
            os,
            host_name,
            cpu,
            memory,
            disks,
            hints,
        }
    }
}

/// What a hint is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HintKind {
    /// Sticks of different speeds: all run at the slowest.
    MismatchedSpeed,
    /// Sticks run slower than they are rated for (an XMP/EXPO profile is off).
    BelowRated,
    /// Sticks of different sizes.
    MismatchedSize,
    /// One channel in use: adding a matching stick would double the bandwidth.
    SingleChannel,
    /// Two or more channels in use.
    MultiChannel,
    /// Empty slots remain.
    FreeSlots,
    /// The board's capacity limit.
    MaxCapacity,
    /// Nothing to upgrade.
    NotUpgradeable,
    /// The layout could not be read.
    Unknown,
}

/// One plain-language observation about the memory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hint {
    /// What it is about.
    pub kind: HintKind,
    /// The sentence to show.
    pub message: String,
    /// The slot it concerns, when one.
    pub slot: Option<String>,
}

impl Hint {
    fn new(kind: HintKind, message: impl Into<String>, slot: Option<String>) -> Hint {
        Hint {
            kind,
            message: message.into(),
            slot,
        }
    }
}

/// The channel letter a slot or bank label names: "ChannelA-DIMM0" and "DIMM_A1" give 'A'. `None` when the label does
/// not say.
fn channel_letter(label: &str) -> Option<char> {
    let up = label.to_ascii_uppercase();
    if let Some(i) = up.find("CHANNEL") {
        let rest = up[i + 7..].trim_start_matches([' ', '_', '-']);
        if let Some(c) = rest.chars().next().filter(|c| c.is_ascii_alphanumeric()) {
            return Some(c);
        }
    }
    // "DIMM_A1", "DIMM A2", "DIMMB1": a letter directly before the slot digit.
    let chars: Vec<char> = up.chars().collect();
    for w in chars.windows(2) {
        if w[0].is_ascii_alphabetic() && w[1].is_ascii_digit() {
            let idx = up.find(w[0]).unwrap_or(0);
            // Not the tail of a word such as "DIMM1": the letter must follow a separator or "DIMM".
            let before = &up[..idx];
            if before.is_empty() || before.ends_with(['_', ' ', '-']) || before.ends_with("DIMM") {
                // "DIMM1" has 'M' before the digit; reject letters that continue the word DIMM.
                if !(before.ends_with("DIM") && w[0] == 'M') {
                    return Some(w[0]);
                }
            }
        }
    }
    None
}

/// How many memory channels the populated sticks span, or `None` when the slot labels do not say.
pub fn channels(array: &MemoryArray) -> Option<u32> {
    if array.modules.is_empty() {
        return None;
    }
    let mut seen: Vec<char> = Vec::new();
    for m in &array.modules {
        let c = channel_letter(&m.slot).or_else(|| m.bank.as_deref().and_then(channel_letter))?;
        if !seen.contains(&c) {
            seen.push(c);
        }
    }
    Some(seen.len() as u32)
}

/// A free slot to suggest, derived from the populated labels ("DIMM_A1" -> "DIMM_B1"); `None` when the pattern is
/// not recognisable.
fn suggest_slot(array: &MemoryArray) -> Option<String> {
    let first = array.modules.first()?;
    let label = &first.slot;
    let c = channel_letter(label)?;
    let next = char::from_u32(u32::from(c) + 1).filter(|n| n.is_ascii_alphabetic())?;
    let up = label.to_ascii_uppercase();
    let at = up.rfind(c)?;
    let mut out = String::new();
    out.push_str(&label[..at]);
    out.push(if label.as_bytes().get(at).is_some_and(|b| b.is_ascii_lowercase()) {
        next.to_ascii_lowercase()
    } else {
        next
    });
    out.push_str(&label[at + 1..]);
    let taken = array.modules.iter().any(|m| m.slot.eq_ignore_ascii_case(&out));
    (!taken).then_some(out)
}

fn gib(bytes: u64) -> String {
    let g = bytes as f64 / (1u64 << 30) as f64;
    if (g - g.round()).abs() < 0.05 {
        format!("{} GB", g.round() as u64)
    } else {
        format!("{g:.1} GB")
    }
}

/// Plain-language observations about a memory array: mismatched speeds, a single channel (with the slot to fill),
/// dual channel, free slots and the maximum capacity. Empty for an empty array.
pub fn memory_hints(array: &MemoryArray) -> Vec<Hint> {
    let mut out = Vec::new();
    let mods = &array.modules;
    if mods.is_empty() {
        return out;
    }
    let rated: Vec<u32> = mods.iter().filter_map(|m| m.speed_mts).filter(|s| *s > 0).collect();
    let running: Vec<u32> = mods.iter().filter_map(MemoryModule::running_mts).filter(|s| *s > 0).collect();
    let (rmin, rmax) = (rated.iter().min().copied(), rated.iter().max().copied());
    if let (Some(lo), Some(hi)) = (rmin, rmax) {
        if lo != hi {
            out.push(Hint::new(
            HintKind::MismatchedSpeed,
            format!("Your sticks are rated for different speeds ({lo} and {hi} MT/s), so they all run at the slowest. Matching sticks would let the faster ones use their full speed."),
            None,
        ));
        }
    }
    if let (Some(run), Some(max_rated)) = (running.iter().max().copied(), rmax) {
        // Everything runs below what the fastest stick is rated for, and the sticks agree with each other.
        if run + 100 < max_rated && rmin == rmax {
            out.push(Hint::new(
                HintKind::BelowRated,
                format!("Your memory is rated for {max_rated} MT/s but runs at {run} MT/s. Turning on its XMP or EXPO profile in the firmware settings may speed it up."),
                None,
            ));
        }
    }
    let sizes: Vec<u64> = mods.iter().map(|m| m.capacity_bytes).collect();
    if sizes.iter().any(|s| *s != sizes[0]) {
        out.push(Hint::new(
            HintKind::MismatchedSize,
            "Your sticks are different sizes, which can limit dual-channel mode.",
            None,
        ));
    }
    match channels(array) {
        Some(1) if array.slots >= 2 || array.slots == 0 => {
            let slot = suggest_slot(array);
            let msg = match &slot {
                Some(s) => format!("Memory runs on a single channel. Add a matching stick in {s} to double the bandwidth."),
                None => "Memory runs on a single channel. Add a matching stick in the paired slot (see your board's manual) to double the bandwidth."
                    .into(),
            };
            out.push(Hint::new(HintKind::SingleChannel, msg, slot));
        }
        Some(n) if n >= 2 => {
            out.push(Hint::new(
                HintKind::MultiChannel,
                format!("Your memory runs in {n}-channel mode."),
                None,
            ));
        }
        _ => {}
    }
    let free = array.free_slots();
    if free > 0 {
        out.push(Hint::new(
            HintKind::FreeSlots,
            format!(
                "{free} of {} memory slots {} free.",
                array.slots,
                if free == 1 { "is" } else { "are" }
            ),
            None,
        ));
    }
    if let Some(max) = array.max_capacity_bytes.filter(|m| *m > 0) {
        let installed = array.installed_bytes();
        let msg = if installed >= max {
            format!("You have the most memory this board supports ({}).", gib(max))
        } else {
            format!("This board supports up to {}; {} is installed.", gib(max), gib(installed))
        };
        out.push(Hint::new(HintKind::MaxCapacity, msg, None));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn module(slot: &str, gb: u64, rated: u32, running: u32) -> MemoryModule {
        MemoryModule {
            slot: slot.into(),
            bank: None,
            capacity_bytes: gb << 30,
            speed_mts: Some(rated),
            configured_speed_mts: Some(running),
            kind: MemoryKind::Ddr4,
            form_factor: FormFactor::Dimm,
            manufacturer: Some("Acme".into()),
            part_number: Some("AC-0001".into()),
        }
    }

    fn array(slots: u32, mods: Vec<MemoryModule>) -> MemoryArray {
        MemoryArray {
            slots,
            max_capacity_bytes: Some(64 << 30),
            modules: mods,
        }
    }

    fn kinds(h: &[Hint]) -> Vec<HintKind> {
        h.iter().map(|h| h.kind).collect()
    }

    #[test]
    fn smbios_codes_and_names() {
        assert_eq!(MemoryKind::from_smbios(26), MemoryKind::Ddr4);
        assert_eq!(MemoryKind::from_smbios(34), MemoryKind::Ddr5);
        assert_eq!(MemoryKind::from_smbios(24), MemoryKind::Ddr3);
        assert_eq!(MemoryKind::from_smbios(19), MemoryKind::Ddr2);
        assert_eq!(MemoryKind::from_smbios(27), MemoryKind::Lpddr3);
        assert_eq!(MemoryKind::from_smbios(28), MemoryKind::Lpddr4);
        assert_eq!(MemoryKind::from_smbios(35), MemoryKind::Lpddr5);
        assert_eq!(MemoryKind::from_smbios(0), MemoryKind::Unknown);
        assert_eq!(MemoryKind::from_smbios(999), MemoryKind::Other);
        assert_eq!(MemoryKind::from_name(" lpddr5x "), MemoryKind::Lpddr5);
        assert_eq!(MemoryKind::from_name("LPDDR4X"), MemoryKind::Lpddr4);
        assert_eq!(MemoryKind::from_name("LPDDR3"), MemoryKind::Lpddr3);
        assert_eq!(MemoryKind::from_name("DDR5"), MemoryKind::Ddr5);
        assert_eq!(MemoryKind::from_name("ddr4"), MemoryKind::Ddr4);
        assert_eq!(MemoryKind::from_name("DDR3"), MemoryKind::Ddr3);
        assert_eq!(MemoryKind::from_name("DDR2"), MemoryKind::Ddr2);
        assert_eq!(MemoryKind::from_name("SDRAM"), MemoryKind::Other);
        assert_eq!(MemoryKind::from_name(""), MemoryKind::Unknown);
        for k in [
            MemoryKind::Ddr2,
            MemoryKind::Ddr3,
            MemoryKind::Ddr4,
            MemoryKind::Ddr5,
            MemoryKind::Lpddr3,
            MemoryKind::Lpddr4,
            MemoryKind::Lpddr5,
            MemoryKind::Other,
            MemoryKind::Unknown,
        ] {
            assert!(!k.label().is_empty());
        }
        assert_eq!(FormFactor::from_smbios(8), FormFactor::Dimm);
        assert_eq!(FormFactor::from_smbios(12), FormFactor::SoDimm);
        assert_eq!(FormFactor::from_smbios(11), FormFactor::Soldered);
        assert_eq!(FormFactor::from_smbios(0), FormFactor::Unknown);
        assert_eq!(FormFactor::from_smbios(6), FormFactor::Other);
    }

    #[test]
    fn channel_detection() {
        let a = array(
            4,
            vec![module("ChannelA-DIMM0", 8, 3200, 3200), module("ChannelB-DIMM0", 8, 3200, 3200)],
        );
        assert_eq!(channels(&a), Some(2));
        let b = array(4, vec![module("DIMM_A1", 8, 3200, 3200), module("DIMM_A2", 8, 3200, 3200)]);
        assert_eq!(channels(&b), Some(1));
        let c = array(2, vec![module("DIMM1", 8, 3200, 3200)]);
        assert_eq!(channels(&c), None);
        let mut d = array(2, vec![module("X", 8, 3200, 3200)]);
        d.modules[0].bank = Some("Channel B".into());
        assert_eq!(channels(&d), Some(1));
        assert_eq!(channels(&array(2, vec![])), None);
        assert_eq!(channel_letter("Controller0-ChannelA-DIMM0"), Some('A'));
        assert_eq!(channel_letter("DIMM A2"), Some('A'));
        assert_eq!(channel_letter("SODIMM"), None);
        assert_eq!(channel_letter("DIMM1"), None);
        assert_eq!(channel_letter("DIMM_B2"), Some('B'));
        assert_eq!(channel_letter("channel"), None);
    }

    #[test]
    fn single_channel_suggests_a_slot() {
        let a = array(4, vec![module("DIMM_A1", 8, 3200, 3200)]);
        let h = memory_hints(&a);
        let s = h.iter().find(|h| h.kind == HintKind::SingleChannel).unwrap();
        assert_eq!(s.slot.as_deref(), Some("DIMM_B1"));
        assert!(s.message.contains("DIMM_B1"));
        assert!(kinds(&h).contains(&HintKind::FreeSlots));
        assert!(kinds(&h).contains(&HintKind::MaxCapacity));
        // A lower-case label keeps its case; an unrecognisable one gets the generic text.
        let l = array(2, vec![module("dimm_a1", 8, 3200, 3200)]);
        assert_eq!(
            memory_hints(&l)
                .iter()
                .find(|h| h.kind == HintKind::SingleChannel)
                .unwrap()
                .slot
                .as_deref(),
            Some("dimm_b1")
        );
        let mut g = array(2, vec![module("DIMM_A1", 8, 3200, 3200)]);
        g.modules[0].slot = "Channel Z".into();
        let hz = memory_hints(&g);
        assert!(hz.iter().find(|h| h.kind == HintKind::SingleChannel).unwrap().slot.is_none());
        // A single-slot machine cannot add a stick.
        let one = array(1, vec![module("DIMM_A1", 8, 3200, 3200)]);
        assert!(!kinds(&memory_hints(&one)).contains(&HintKind::SingleChannel));
    }

    #[test]
    fn dual_channel_and_full() {
        let mut a = array(
            2,
            vec![module("ChannelA-DIMM0", 32, 3200, 3200), module("ChannelB-DIMM0", 32, 3200, 3200)],
        );
        let h = memory_hints(&a);
        assert_eq!(kinds(&h), vec![HintKind::MultiChannel, HintKind::MaxCapacity]);
        assert!(h[1].message.contains("most memory"));
        assert!(h[0].message.contains("2-channel"));
        a.max_capacity_bytes = None;
        assert_eq!(kinds(&memory_hints(&a)), vec![HintKind::MultiChannel]);
        a.slots = 3;
        let h = memory_hints(&a);
        assert!(h
            .iter()
            .any(|h| h.kind == HintKind::FreeSlots && h.message.starts_with("1 of 3 memory slots is free")));
        a.slots = 4;
        assert!(memory_hints(&a).iter().any(|h| h.message.contains("are free")));
    }

    #[test]
    fn speed_and_size_mismatches() {
        let a = array(
            4,
            vec![module("ChannelA-DIMM0", 8, 3200, 2666), module("ChannelB-DIMM0", 16, 2666, 2666)],
        );
        let h = memory_hints(&a);
        assert!(kinds(&h).contains(&HintKind::MismatchedSpeed));
        assert!(kinds(&h).contains(&HintKind::MismatchedSize));
        assert!(!kinds(&h).contains(&HintKind::BelowRated));
        let slow = array(
            2,
            vec![module("ChannelA-DIMM0", 8, 3200, 2133), module("ChannelB-DIMM0", 8, 3200, 2133)],
        );
        let h = memory_hints(&slow);
        let b = h.iter().find(|h| h.kind == HintKind::BelowRated).unwrap();
        assert!(b.message.contains("3200") && b.message.contains("2133"));
        let mut none = module("A", 8, 0, 0);
        none.speed_mts = None;
        none.configured_speed_mts = None;
        assert!(!kinds(&memory_hints(&array(2, vec![none]))).contains(&HintKind::BelowRated));
        assert!(memory_hints(&array(2, vec![])).is_empty());
        let part = array(2, vec![module("ChannelA-DIMM0", 12, 3200, 3200)]);
        assert!(memory_hints(&part).iter().any(|h| h.message.contains("12 GB")));
        let mut odd = part.clone();
        odd.modules[0].capacity_bytes = (3 << 30) / 2;
        odd.max_capacity_bytes = Some(8 << 30);
        assert!(memory_hints(&odd).iter().any(|h| h.message.contains("1.5 GB")));
    }

    #[test]
    fn layouts() {
        let u = MemoryLayout::unified(16 << 30, MemoryKind::Lpddr5, None);
        assert_eq!(u.total(), 16 << 30);
        assert_eq!(kinds(&u.hints()), vec![HintKind::NotUpgradeable]);
        let t = MemoryLayout::TotalOnly {
            total: 8 << 30,
            reason: Unavailable::linux_dmi_root_only(),
        };
        assert_eq!(t.total(), 8 << 30);
        assert_eq!(kinds(&t.hints()), vec![HintKind::Unknown]);
        let linux = Unavailable::linux_dmi_root_only();
        assert!(linux.reason.contains("administrator") && linux.reason.contains("total only"));
        assert!(
            !linux.reason.to_lowercase().contains("helper"),
            "the helper cannot read it, so the text must not offer it"
        );
        let s = MemoryLayout::Slots(array(2, vec![module("ChannelA-DIMM0", 8, 3200, 3200)]));
        assert_eq!(s.total(), 8 << 30);
        assert!(!s.hints().is_empty());
    }

    #[test]
    fn bounded_and_free() {
        let many = (0..200).map(|i| module(&format!("S{i}"), 1, 1, 1)).collect();
        let a = array(u32::MAX, many).bounded();
        assert_eq!(a.modules.len(), MAX_MODULES);
        assert_eq!(a.slots, MAX_MODULES as u32 * 4);
        assert_eq!(array(0, vec![module("A", 1, 1, 1)]).free_slots(), 0);
    }

    #[test]
    fn serde_round_trips() {
        let layouts = [
            MemoryLayout::Slots(array(2, vec![module("ChannelA-DIMM0", 8, 3200, 3200)])),
            MemoryLayout::unified(8 << 30, MemoryKind::Lpddr4, Some("Acme".into())),
            MemoryLayout::TotalOnly {
                total: 1,
                reason: Unavailable::failed("no"),
            },
        ];
        for l in layouts {
            let inv = SystemInventory::new(
                "OS".into(),
                Some("host".into()),
                CpuInfo::default(),
                l,
                vec![DiskInfo {
                    name: "d".into(),
                    total_bytes: 1,
                    removable: false,
                }],
            );
            let j = serde_json::to_string(&inv).unwrap();
            assert_eq!(serde_json::from_str::<SystemInventory>(&j).unwrap(), inv);
            let p = postcard::to_stdvec(&inv).unwrap();
            assert_eq!(postcard::from_bytes::<SystemInventory>(&p).unwrap(), inv);
        }
        let j = serde_json::to_value(MemoryLayout::unified(1, MemoryKind::Ddr5, None)).unwrap();
        assert_eq!(j["unified"]["upgradeable"], false);
        assert_eq!(j["unified"]["kind"], "ddr5");
        let m = serde_json::to_value(module("A", 1, 1, 1)).unwrap();
        assert_eq!(m["capacityBytes"], 1u64 << 30);
        assert_eq!(m["formFactor"], "dimm");
    }
}
