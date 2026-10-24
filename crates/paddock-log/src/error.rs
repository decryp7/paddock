use std::fmt;

/// What can go wrong installing the log. Messages say what to fix; none contains log content.
#[derive(Debug)]
pub enum LogError {
    /// A `tracing` subscriber is already installed in this process (the log installs once).
    SubscriberTaken,
    /// Another `log` logger is already installed in this process.
    LoggerTaken,
}

impl fmt::Display for LogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LogError::SubscriberTaken => write!(
                f,
                "a tracing subscriber is already installed; install the Paddock log first and once"
            ),
            LogError::LoggerTaken => write!(
                f,
                "another logger is already installed; install the Paddock log first and once"
            ),
        }
    }
}

impl std::error::Error for LogError {}

/// Result of installing the log.
pub type Result<T> = std::result::Result<T, LogError>;

/// Locks `m`, recovering the data when another thread panicked while holding it (a log must keep working after a panic).
pub(crate) fn lock<T>(m: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_poisoned_lock_still_opens() {
        let m = std::sync::Arc::new(std::sync::Mutex::new(1));
        let m2 = m.clone();
        let _ = std::thread::spawn(move || {
            let _g = m2.lock().unwrap();
            panic!("poison it");
        })
        .join();
        assert!(m.is_poisoned());
        *super::lock(&m) += 1;
        assert_eq!(*super::lock(&m), 2);
        assert!(
            super::LogError::SubscriberTaken
                .to_string()
                .contains("already installed")
        );
        assert!(
            super::LogError::LoggerTaken
                .to_string()
                .contains("another logger")
        );
    }
}
