//! The resumable, verified downloader that the FFmpeg runtime and the research browser's download use: the download
//! port, its progress, free-space checks and the segmented transfer. It leaves with the move onto `paddock-download`
//! (ADR 0070 D5).

pub mod disk;
pub mod download;
pub mod priority;
pub mod segmented;
pub mod source;

pub use disk::{DiskSpace, SystemDisk};
pub use download::{DownloadOptions, Downloader};
pub use segmented::HostGate;
pub use source::{ModelSource, Phase, Progress, ProgressSnapshot, RemoteFile};
