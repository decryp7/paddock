//! Pure parsers and mappings for what the OS and its tools return. Compiled on every OS so they are tested
//! everywhere; the OS modules only feed them bytes and text.

pub mod cpu_topology;
pub mod desktop_entry;
pub mod gpu;
pub mod ioreg;
pub mod launchd;
pub mod linux_hw;
pub mod linux_net;
pub mod linux_proc;
pub mod linux_tools;
pub mod lnk;
pub mod lsof;
pub mod mac_tools;
pub mod powercfg;
pub mod startup_approved;
pub mod time;
pub mod win_facts;
