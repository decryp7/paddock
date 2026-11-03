//! Pure parsers and mappings for what the OS and its tools return. Compiled on every OS so they are tested
//! everywhere; the OS modules only feed them bytes and text.

pub mod btm;
pub mod cpu_topology;
pub mod desktop_entry;
pub mod gpu;
pub mod hardware;
pub mod ioreg;
pub mod launchd;
pub mod linux_hw;
pub mod linux_net;
pub mod linux_proc;
pub mod linux_tools;
pub mod lnk;
pub mod lsof;
pub mod mac_pcblist;
pub mod mac_tools;
pub mod pdh_list;
pub mod powercfg;
pub mod sock_diag;
pub mod socket_owner;
pub mod startup_approved;
pub mod time;
pub mod wifi;
pub mod win_estats;
pub mod win_facts;
pub mod win_tcp;
