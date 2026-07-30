use std::cell::RefCell;

thread_local! {
    /// Global buffer for log lines. Every `log_event!` call pushes a formatted
    /// line here so it can be flushed to the GitHub log file atomically with the
    /// pending-item commit.
    ///
    /// Uses `RefCell` (not `Mutex`) because Cloudflare Workers run on WASM with
    /// a single thread — no atomics needed, and `Mutex` is unavailable.
    pub(crate) static LOG_BUFFER: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };

    /// Whether log lines should be written to the GitHub log file.
    /// Controlled by the `LOG_TO_FILE` env variable (default: true).
    /// When disabled, `log_event!` still prints to `console_log!` but does
    /// NOT buffer lines for the GitHub log file.
    pub(crate) static LOG_ENABLED: RefCell<bool> = const { RefCell::new(true) };
}

/// Sets whether log lines are written to the GitHub log file.
/// Call this at the start of each request based on the `LOG_TO_FILE` env var.
pub fn set_log_enabled(enabled: bool) {
    LOG_ENABLED.with(|flag| {
        *flag.borrow_mut() = enabled;
    });
}

/// Drains the log buffer and returns all accumulated lines.
/// The buffer is cleared after this call.
pub fn take_logs() -> Vec<String> {
    let mut result = Vec::new();
    LOG_BUFFER.with(|buf| {
        let mut buf = buf.borrow_mut();
        result = buf.clone();
        buf.clear();
    });
    result
}

/// Restores previously taken log lines back into the buffer.
pub fn restore_logs(lines: &[String]) {
    if lines.is_empty() {
        return;
    }
    LOG_BUFFER.with(|buf| {
        let mut buf = buf.borrow_mut();
        buf.extend(lines.iter().cloned());
    });
}

/// Backward-compatible alias used by older callers.
#[allow(dead_code)]
pub fn flush_logs() -> Vec<String> {
    take_logs()
}

pub fn format_log_line(
    level: impl AsRef<str>,
    name: impl AsRef<str>,
    message: impl AsRef<str>,
) -> String {
    let timestamp = chrono::Utc::now()
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string();
    let level = level.as_ref();
    let name = name.as_ref();
    let message = message.as_ref();
    let base = format!("{} [{}] {} - {}", timestamp, level, name, message);

    if level.eq_ignore_ascii_case("error")
        && (name.starts_with("github.") || name.starts_with("telegram."))
    {
        let header = format!("=== {} {} ===", level.to_uppercase(), name);
        let separator = "=".repeat(header.len());
        format!("{}\n{}\n{}", header, base, separator)
    } else {
        base
    }
}

#[cfg(test)]
mod tests {
    use super::format_log_line;

    #[test]
    fn format_log_line_should_wrap_github_and_telegram_errors_in_blocks() {
        let line = format_log_line(
            "error",
            "github.api.post_failed",
            "status=500 body=bad gateway",
        );
        assert!(line.contains("=== ERROR github.api.post_failed ==="));
        assert!(line.contains("github.api.post_failed"));
        assert!(line.contains("status=500 body=bad gateway"));
    }

    #[test]
    fn take_and_restore_logs_should_round_trip() {
        super::set_log_enabled(true);
        crate::log_event!("info", "logger.test", "round_trip");
        let lines = super::take_logs();
        assert_eq!(lines.len(), 1);
        super::restore_logs(&lines);
        let restored = super::take_logs();
        assert_eq!(restored.len(), 1);
        assert!(restored[0].contains("logger.test"));
    }
}

#[macro_export]
macro_rules! log_event {
    ($level:expr, $name:expr) => {
        $crate::log_event!($level, $name, "");
    };
    ($level:expr, $name:expr, $($arg:tt)*) => {
        {
            let message = format!($($arg)*);
            let line = $crate::logger::format_log_line($level, $name, &message);
            // Always print to the Worker console for immediate visibility.
            #[cfg(target_arch = "wasm32")]
            ::worker::console_log!("{}", line);
            #[cfg(not(target_arch = "wasm32"))]
            let _ = &line;
            // Buffer for GitHub log file only if file logging is enabled.
            $crate::logger::LOG_ENABLED.with(|flag| {
                if *flag.borrow() {
                    $crate::logger::LOG_BUFFER.with(|buf| {
                        if let Ok(mut buf) = buf.try_borrow_mut() {
                            buf.push(line);
                        }
                    });
                }
            });
        }
    };
}