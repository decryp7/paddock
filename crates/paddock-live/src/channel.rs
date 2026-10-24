//! The name of a feed.

use std::fmt;

/// Longest channel name.
pub const MAX_LEN: usize = 32;

/// A feed's name: 1 to 32 lowercase letters, digits and dashes, starting with a letter. Parsed once at the edge (the IPC
/// argument of `live_snapshot`), so an event name built from it is always valid.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Channel(String);

/// Why a name is not a channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelError {
    /// Empty or longer than [`MAX_LEN`].
    Length,
    /// Does not start with a lowercase letter.
    Start,
    /// Has a character other than lowercase letters, digits and dashes.
    Character,
}

impl fmt::Display for ChannelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Length => "a live channel name has 1 to 32 characters",
            Self::Start => "a live channel name starts with a lowercase letter",
            Self::Character => "a live channel name has only lowercase letters, digits and dashes",
        })
    }
}

impl std::error::Error for ChannelError {}

impl Channel {
    /// Checks `name`.
    pub fn new(name: &str) -> Result<Channel, ChannelError> {
        if name.is_empty() || name.len() > MAX_LEN {
            return Err(ChannelError::Length);
        }
        if !name.starts_with(|c: char| c.is_ascii_lowercase()) {
            return Err(ChannelError::Start);
        }
        if !name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            return Err(ChannelError::Character);
        }
        Ok(Channel(name.to_string()))
    }

    /// The name.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The event the channel's frames are emitted on: `paddock://live/<name>`.
    pub fn event(&self) -> String {
        format!("paddock://live/{}", self.0)
    }
}
