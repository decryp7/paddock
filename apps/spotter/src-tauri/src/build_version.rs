use std::fmt::{Display, Formatter};
use regex::{Captures, Regex};
use tracing::error;

#[derive(Debug, Default, Copy, Clone, Eq, PartialEq)]
pub struct BuildVersion {
    pub major: i32,
    pub minor: i32,
    pub patch: i32,
    pub t: i32
}

impl BuildVersion {
    pub fn parse(version: &str) -> Result<BuildVersion, anyhow::Error> {
        let version_regex = Regex::new(r".*(R\d).(\d{2}).(\d{2})(T\d{2}).*").unwrap();
        let binding = version.to_uppercase();
        match version_regex.captures(binding.as_str()) {
            None => {
                error!("Unable to parse {}.", binding);
            }
            Some(c) => {
                //println!("{}", c.len());
                if c.len() == 5 {
                    //println!("{} {} {} {}", c[1].to_string(), c[2].to_string(), c[3].to_string(), c[4].to_string());
                    return Ok(BuildVersion {
                        major: c[1].replace('R', "").parse()?,
                        minor: c[2].parse()?,
                        patch: c[3].parse()?,
                        t: c[4].replace('T', "").parse()?
                    })
                }
            }
        }

        Ok(BuildVersion { major: 0, minor: 0, patch: 0, t: 0 })
    }
}

impl Display for BuildVersion {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "R{}.{:0>2}.{:0>2}T{:0>2}", self.major, self.minor, self.patch, self.t)
    }
}