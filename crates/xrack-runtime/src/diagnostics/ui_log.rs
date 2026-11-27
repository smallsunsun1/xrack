/// Event levels used by logging statements in XRack UI code.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LogLevel {
    Trace,
    Debug,
    #[default]
    Info,
    Warn,
    Error,
}

/// Emit an XRack UI event through the application's tracing subscriber.
pub fn log_ui(
    level: LogLevel,
    message: &str,
    component: &str,
    file: &str,
    line: usize,
    column: usize,
) {
    macro_rules! emit {
        ($level:ident) => {
            tracing::event!(
                target: "xrack::ui",
                tracing::Level::$level,
                component,
                file,
                line,
                column,
                message
            )
        };
    }
    match level {
        LogLevel::Trace => emit!(TRACE),
        LogLevel::Debug => emit!(DEBUG),
        LogLevel::Info => emit!(INFO),
        LogLevel::Warn => emit!(WARN),
        LogLevel::Error => emit!(ERROR),
    }
}
