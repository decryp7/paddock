use std::{env, fs};
use std::fs::{metadata, OpenOptions};
use std::io::Write;
use anyhow::Error;
use tracing::{error, info};
use crate::build_version::BuildVersion;

pub trait VersionUpdater{
    fn get_version(&self) -> BuildVersion;
    fn set_version(&self, version: BuildVersion);
    fn reset(&self);
}

pub struct FileCacheVersionUpdater {
    path: String,
}

impl FileCacheVersionUpdater {
    pub(crate) fn new(path: &str) -> Self {
        Self {path: path.into()}
    }
}

impl VersionUpdater for FileCacheVersionUpdater {
    fn get_version(&self) -> BuildVersion {
        if metadata(&self.path).is_ok() {
            let s = fs::read_to_string(&self.path).unwrap();
            match BuildVersion::parse(&s) {
                Ok(v) => {
                    println!("{}", v);
                    return v;
                }
                Err(e) => {
                    error!("Unable to parse version {}. Error: {}", s, e);
                }
            }
        }

        BuildVersion::default()
    }

    fn set_version(&self, version: BuildVersion) {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&self.path)
            .unwrap();
        match file.write_all(version.to_string().as_bytes()) {
            Ok(_) => {
                info!("Wrote {} to {}.",version, env::current_dir().unwrap().into_os_string().into_string().unwrap());
            }
            Err(e) => {
                error!("Unable to write version to {}. Error: {}", &self.path, e);
            }
        }
    }

    fn reset(&self) {
        if metadata(&self.path).is_ok() {
            match fs::remove_file(&self.path) {
                Ok(_) => {
                    info!("Removed file {}.", &self.path);
                }
                Err(e) => {
                    error!("Failed to remove file {}. Error: {}", &self.path, e);
                }
            }
        }
    }
}