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
pub fn flush_logs() -> Vec<String> {
    let mut result = Vec::new();
    LOG_BUFFER.with(|buf| {
        let mut buf = buf.borrow_mut();
        result = buf.clone();
        buf.clear();
    });
    result
}

#[macro_export]
macro_rules! log_event {
    ($level:expr, $name:expr) => {
        $crate::log_event!($level, $name, "");
    };
    ($level:expr, $name:expr, $($arg:tt)*) => {
        {
            let message = format!($($arg)*);
            let timestamp = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
            let line = format!("{} [{}] {} - {}", timestamp, $level, $name, message);
            // Always print to the Worker console for immediate visibility.
            ::worker::console_log!("{}", line);
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