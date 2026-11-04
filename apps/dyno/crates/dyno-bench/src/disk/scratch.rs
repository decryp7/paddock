//! The one scratch file of the disk test, made so that it cannot be left behind. `create_new` inside the folder the agent made
//! refuses to touch anything that exists. On Unix the name is removed right after opening (the open descriptor keeps the file,
//! and the space returns when the process ends, even when it is killed); on Windows the file is opened with
//! delete-on-close, which the kernel honours when the process dies. The file system's cache is bypassed (Linux `O_DIRECT`,
//! Windows no-buffering and write-through, macOS `F_NOCACHE`); where the file system refuses that, the test runs buffered,
//! evicts pages after each flush where it can, and says "cached reads possible".
#![cfg_attr(
    not(test),
    deny(
        clippy::let_underscore_must_use,
        clippy::let_underscore_future,
        clippy::let_underscore_untyped
    )
)]

use super::driver::{BlockIo, ALIGN};
use super::sys;
use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;

/// Name of the scratch file inside the caller's folder (it exists only for an instant on Unix).
pub const SCRATCH_NAME: &str = "dyno-bench.tmp";

/// How the file is read and written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// The cache is bypassed.
    Direct,
    /// The file system refused direct I/O: buffered, evicted after flushes where the OS allows.
    Buffered,
}

/// The open scratch file.
pub struct Scratch {
    file: File,
    mode: Mode,
    dir: std::path::PathBuf,
}

#[cfg(windows)]
mod win {
    pub const FILE_FLAG_WRITE_THROUGH: u32 = 0x8000_0000;
    pub const FILE_FLAG_NO_BUFFERING: u32 = 0x2000_0000;
    pub const FILE_FLAG_DELETE_ON_CLOSE: u32 = 0x0400_0000;
    pub const GENERIC_READ: u32 = 0x8000_0000;
    pub const GENERIC_WRITE: u32 = 0x4000_0000;
    pub const DELETE: u32 = 0x0001_0000;
}

fn open(path: &Path, direct: bool) -> io::Result<File> {
    let mut o = OpenOptions::new();
    o.read(true).write(true).create_new(true);
    #[cfg(target_os = "linux")]
    if direct {
        use std::os::unix::fs::OpenOptionsExt;
        o.custom_flags(libc::O_DIRECT);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        let flags = win::FILE_FLAG_DELETE_ON_CLOSE
            | if direct {
                win::FILE_FLAG_NO_BUFFERING | win::FILE_FLAG_WRITE_THROUGH
            } else {
                0
            };
        o.access_mode(win::GENERIC_READ | win::GENERIC_WRITE | win::DELETE)
            .custom_flags(flags);
    }
    let f = o.open(path)?;
    #[cfg(target_os = "macos")]
    if direct {
        sys::no_cache(&f)?;
    }
    let _: bool = direct;
    Ok(f)
}

/// Whether an error means "this file system does not do direct I/O" (Linux `EINVAL`, Windows `ERROR_INVALID_PARAMETER`).
/// Removes the scratch name after a refused attempt: a name that is not there is the goal; any other failure is said on
/// standard error (the bench prints its result on standard output), so a left-over file can be explained.
fn remove_scratch(path: &Path) {
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => eprintln!("dyno-bench: the scratch file {} could not be removed: {e}", path.display()),
    }
}

fn refused_direct(e: &io::Error) -> bool {
    matches!(e.raw_os_error(), Some(22) | Some(87)) || e.kind() == io::ErrorKind::InvalidInput
}

impl Scratch {
    /// Makes the scratch file in `dir`. Fails if the name exists; never overwrites.
    pub fn create(dir: &Path) -> io::Result<Scratch> {
        let path = dir.join(SCRATCH_NAME);
        let try_mode = |direct: bool| -> io::Result<File> {
            let f = open(&path, direct)?;
            if direct {
                // Linux says what alignment the file wants (kernel 6.1+); one our 4 KiB buffers cannot meet means no direct I/O here.
                if let Some((mem, off)) = sys::dio_align(&f) {
                    if !sys::align_fits(mem, off) {
                        drop(f);
                        remove_scratch(&path);
                        return Err(io::Error::from_raw_os_error(22));
                    }
                }
                // A probe: some file systems open with O_DIRECT and refuse the first aligned write.
                let mut probe = super::driver::AlignedBuf::new(ALIGN, ALIGN);
                if let Err(e) = write_at(&f, probe.bytes(), 0) {
                    drop(f);
                    remove_scratch(&path);
                    return Err(e);
                }
            }
            Ok(f)
        };
        let (file, mode) = match try_mode(true) {
            Ok(f) => (f, Mode::Direct),
            Err(e) if refused_direct(&e) => {
                // EINVAL is never "already exists", so a file at this name is the one this attempt made (Linux creates it before it
                // refuses O_DIRECT on tmpfs).
                remove_scratch(&path);
                (try_mode(false)?, Mode::Buffered)
            }
            // Anything else (an existing file, no space, no rights) goes to the caller untouched: `create_new` either made the file
            // and the probe above removed it, or it made nothing, and a file that was already there is never removed.
            Err(e) => return Err(e),
        };
        // Unix: the name goes now, so nothing can be left behind whatever happens next.
        #[cfg(unix)]
        std::fs::remove_file(&path)?;
        Ok(Scratch {
            file,
            mode,
            dir: dir.to_path_buf(),
        })
    }

    /// How the file is accessed.
    pub fn mode(&self) -> Mode {
        self.mode
    }
}

impl super::Drive for Scratch {
    fn direct(&self) -> bool {
        self.mode == Mode::Direct
    }

    fn space(&self) -> Option<(u64, u64)> {
        sys::volume_space(&self.dir)
    }

    /// Flushes to the device, and drops cached pages where the file system would not bypass its cache.
    fn flush(&self) -> io::Result<()> {
        sys::full_sync(&self.file)?;
        if self.mode == Mode::Buffered {
            sys::drop_cache(&self.file);
        }
        Ok(())
    }
}

#[cfg(unix)]
fn read_at(f: &File, buf: &mut [u8], offset: u64) -> io::Result<()> {
    std::os::unix::fs::FileExt::read_exact_at(f, buf, offset)
}

#[cfg(unix)]
fn write_at(f: &File, buf: &[u8], offset: u64) -> io::Result<()> {
    std::os::unix::fs::FileExt::write_all_at(f, buf, offset)
}

#[cfg(windows)]
fn read_at(f: &File, mut buf: &mut [u8], mut offset: u64) -> io::Result<()> {
    use std::os::windows::fs::FileExt;
    while !buf.is_empty() {
        let n = f.seek_read(buf, offset)?;
        if n == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        buf = &mut buf[n..];
        offset += n as u64;
    }
    Ok(())
}

#[cfg(windows)]
fn write_at(f: &File, mut buf: &[u8], mut offset: u64) -> io::Result<()> {
    use std::os::windows::fs::FileExt;
    while !buf.is_empty() {
        let n = f.seek_write(buf, offset)?;
        buf = &buf[n..];
        offset += n as u64;
    }
    Ok(())
}

impl BlockIo for Scratch {
    fn read_at(&self, buf: &mut [u8], offset: u64) -> io::Result<()> {
        read_at(&self.file, buf, offset)
    }

    fn write_at(&self, buf: &[u8], offset: u64) -> io::Result<()> {
        write_at(&self.file, buf, offset)
    }
}
