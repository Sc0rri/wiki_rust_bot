#[macro_export]
macro_rules! log_event {
    ($level:expr, $name:expr) => {
        $crate::log_event!($level, $name, "");
    };
    ($level:expr, $name:expr, $($arg:tt)*) => {
        {
            let message = format!($($arg)*);
            ::worker::console_log!(
                "{}: {} - {}",
                $level,
                $name,
                message
            );
        }
    };
}
