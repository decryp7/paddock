//! The watchdog's timings and limits. The defaults are ADR 0058's (10 s to start, a 500 ms tick, a 3 s pause rule) and
//! plan §11's liveness (a ping every 2 s, five missed in a row: about 10 s without an answer).

use std::fmt;
use std::time::Duration;

/// Timings and limits of the start-up watchdog and the liveness heartbeat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Config {
    /// How long a page may take from starting to load until it reports ready.
    pub ready_timeout: Duration,
    /// How often the ticker looks at the windows.
    pub tick: Duration,
    /// A gap between two ticks longer than this means the app process itself was paused: clocks restart.
    pub pause_gap: Duration,
    /// How often a ready, visible window is pinged.
    pub ping_interval: Duration,
    /// Pings missed in a row before a window is marked unresponsive.
    pub missed_pings: u32,
    /// Reloads after a terminated content process allowed per window within [`Config::terminate_window`].
    pub terminate_reloads: u8,
    /// The window of [`Config::terminate_reloads`].
    pub terminate_window: Duration,
    /// Boot errors logged per window and page load.
    pub max_boot_errors: u32,
    /// Longest boot error message logged, in characters (the rest is cut).
    pub max_error_len: usize,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            ready_timeout: Duration::from_secs(10),
            tick: Duration::from_millis(500),
            pause_gap: Duration::from_secs(3),
            ping_interval: Duration::from_secs(2),
            missed_pings: 5,
            terminate_reloads: 3,
            terminate_window: Duration::from_secs(60),
            max_boot_errors: 10,
            max_error_len: 500,
        }
    }
}

/// Why a [`Config`] was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigError {
    /// The tick is zero.
    ZeroTick,
    /// The pause gap is not longer than the tick: every tick would look like a pause.
    PauseGapNotAboveTick,
    /// The ready timeout or the ping interval is shorter than the tick: it could not be honoured.
    ShorterThanTick,
    /// A ping interval that is not shorter than the pause gap: pings would never be missed (each gap looks like a pause).
    PingIntervalNotBelowPauseGap,
    /// No missed ping, no terminate reload, no boot error or no error text allowed: the limit must be at least one.
    ZeroLimit,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            ConfigError::ZeroTick => "the watchdog tick must be longer than zero",
            ConfigError::PauseGapNotAboveTick => "the pause gap must be longer than the tick",
            ConfigError::ShorterThanTick => {
                "the ready timeout and the ping interval must be at least one tick"
            }
            ConfigError::PingIntervalNotBelowPauseGap => {
                "the ping interval must be shorter than the pause gap"
            }
            ConfigError::ZeroLimit => {
                "missed pings, terminate reloads, boot errors and error length must be at least 1"
            }
        })
    }
}

impl std::error::Error for ConfigError {}

impl Config {
    /// The config if it is consistent.
    pub fn checked(self) -> Result<Config, ConfigError> {
        if self.tick.is_zero() {
            return Err(ConfigError::ZeroTick);
        }
        if self.pause_gap <= self.tick {
            return Err(ConfigError::PauseGapNotAboveTick);
        }
        if self.ready_timeout < self.tick || self.ping_interval < self.tick {
            return Err(ConfigError::ShorterThanTick);
        }
        if self.ping_interval >= self.pause_gap {
            return Err(ConfigError::PingIntervalNotBelowPauseGap);
        }
        if self.missed_pings == 0
            || self.terminate_reloads == 0
            || self.max_boot_errors == 0
            || self.max_error_len == 0
        {
            return Err(ConfigError::ZeroLimit);
        }
        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_are_consistent() {
        assert_eq!(Config::default().checked(), Ok(Config::default()));
    }

    #[test]
    fn inconsistent_timings_are_refused_with_a_reason() {
        let d = Config::default();
        let cases = [
            (
                Config {
                    tick: Duration::ZERO,
                    ..d
                },
                ConfigError::ZeroTick,
            ),
            (
                Config {
                    pause_gap: d.tick,
                    ..d
                },
                ConfigError::PauseGapNotAboveTick,
            ),
            (
                Config {
                    ready_timeout: Duration::from_millis(100),
                    ..d
                },
                ConfigError::ShorterThanTick,
            ),
            (
                Config {
                    ping_interval: Duration::from_millis(100),
                    ..d
                },
                ConfigError::ShorterThanTick,
            ),
            (
                Config {
                    ping_interval: d.pause_gap,
                    ..d
                },
                ConfigError::PingIntervalNotBelowPauseGap,
            ),
            (
                Config {
                    missed_pings: 0,
                    ..d
                },
                ConfigError::ZeroLimit,
            ),
            (
                Config {
                    terminate_reloads: 0,
                    ..d
                },
                ConfigError::ZeroLimit,
            ),
            (
                Config {
                    max_boot_errors: 0,
                    ..d
                },
                ConfigError::ZeroLimit,
            ),
            (
                Config {
                    max_error_len: 0,
                    ..d
                },
                ConfigError::ZeroLimit,
            ),
        ];
        for (config, error) in cases {
            assert_eq!(config.checked(), Err(error));
            assert!(!error.to_string().is_empty());
        }
    }
}
