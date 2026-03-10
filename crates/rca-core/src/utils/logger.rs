/// Logs a message at the FATAL level.
/// In tracing, there is no built-in FATAL level, so this uses the ERROR level
/// with a specific target "FATAL".
#[macro_export]
macro_rules! fatal {
    ($($arg:tt)+) => {
        tracing::error!(target: "FATAL", $($arg)+)
    };
}
