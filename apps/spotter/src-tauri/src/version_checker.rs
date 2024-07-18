use std::fs;
use anyhow::Error;
use regex::Regex;
use crate::build_version::BuildVersion;

pub trait VersionChecker {
    fn get_latest_version(&self) -> Result<BuildVersion, anyhow::Error>;
}

pub struct SharedFolderVersionChecker {
    path: String,
    file_regex: Regex
}

impl SharedFolderVersionChecker {
    pub fn new(path: &str, file_regex: &str) -> Self {
        Self { path: path.into(), file_regex: Regex::new(file_regex).unwrap() }
    }
}

impl VersionChecker for SharedFolderVersionChecker {
    fn get_latest_version(&self) -> Result<BuildVersion, Error> {
        let mut latest_version = BuildVersion::default();
        match fs::read_dir(self.path.as_str()) {
            Ok(directory) => {
                for file in directory {
                    if self.file_regex.is_match(file.as_ref().unwrap().file_name().to_str().unwrap()) {
                        match file.as_ref() {
                            Ok(f) => {
                                let filename = f.file_name();
                                match BuildVersion::parse(filename.to_str().unwrap()) {
                                    Ok(version) => {
                                        //println!("{:?}", version);
                                        if version.major > latest_version.major {
                                            latest_version = version;
                                            continue;
                                        }

                                        if version.major == latest_version.major &&
                                            version.minor > latest_version.minor {
                                            latest_version = version;
                                            continue;
                                        }

                                        if version.major == latest_version.major &&
                                            version.minor == latest_version.minor &&
                                            version.patch > latest_version.patch {
                                            latest_version = version;
                                            continue;
                                        }

                                        if version.major == latest_version.major &&
                                            version.minor == latest_version.minor &&
                                            version.patch == latest_version.patch &&
                                            version.t > latest_version.t {
                                            latest_version = version;
                                        }
                                    }
                                    Err(_) => {}
                                }
                            }
                            Err(_) => {}
                        }
                    }
                }
            }
            Err(_) => {}
        }

        Ok(latest_version)
    }
}