use crate::loggers::{
    Entry, Sink,
    dbgprint::{DbgPrintComponent, DbgPrintLevel},
};
use core::fmt::{self, Write};
use heapless::String;
use log::LevelFilter;

// windows-sys imports DbgPrintEx from ntdll.dll, which a driver cannot load.
windows_link::link!("ntoskrnl.exe" "C" fn DbgPrintEx(componentid: u32, level: u32, format: *const u8, ...) -> u32);

/// How the kernel debugger logger handles records: which records it takes, and under which
/// `DbgPrintEx` component ID and at which `DbgPrintEx` level it prints them.
///
/// The defaults are the logger's level, [`DbgPrintComponent::IHV_DRIVER`] and
/// [`DbgPrintLevel::ERROR`], which a kernel debugger always shows. A record at any other
/// `DbgPrintEx` level is shown only once its component's filter mask enables it; see
/// [`DbgPrintLevel`].
///
/// ```
/// use kernel_logger::kernel_logger::{DbgPrintComponent, DbgPrintLevel, DebugLoggerOptions};
/// use log::LevelFilter;
///
/// // A network driver, whose records are shown once the debugger enables INFO for its
/// // component: `ed nt!Kd_IHVNETWORK_Mask 0x8`. It also takes debug records, whatever the
/// // logger's level.
/// let options = DebugLoggerOptions::new()
///     .with_component(DbgPrintComponent::IHV_NETWORK)
///     .with_dbgprint_level(DbgPrintLevel::INFO)
///     .with_max_level(LevelFilter::Debug);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DebugLoggerOptions {
    max_level: Option<LevelFilter>,
    component: DbgPrintComponent,
    dbgprint_level: DbgPrintLevel,
}

impl DebugLoggerOptions {
    /// The defaults: the logger's level, [`DbgPrintComponent::IHV_DRIVER`] and
    /// [`DbgPrintLevel::ERROR`].
    #[must_use]
    pub const fn new() -> Self {
        Self {
            max_level: None,
            component: DbgPrintComponent::IHV_DRIVER,
            dbgprint_level: DbgPrintLevel::ERROR,
        }
    }

    /// Takes the records up to `level`, instead of those up to the logger's level
    /// ([`KernelLoggerBuilder::with_log_level`](crate::kernel_logger::KernelLoggerBuilder::with_log_level)).
    /// With [`LevelFilter::Off`] it takes none.
    #[must_use]
    pub const fn with_max_level(mut self, level: LevelFilter) -> Self {
        self.max_level = Some(level);
        self
    }

    /// Prints records under `component`.
    #[must_use]
    pub const fn with_component(mut self, component: DbgPrintComponent) -> Self {
        self.component = component;
        self
    }

    /// Prints records at the `DbgPrintEx` level `level`.
    #[must_use]
    pub const fn with_dbgprint_level(mut self, level: DbgPrintLevel) -> Self {
        self.dbgprint_level = level;
        self
    }
}

impl Default for DebugLoggerOptions {
    fn default() -> Self {
        Self::new()
    }
}

/// One line as `DbgPrintEx` prints it: at most 512 bytes.
type Line = String<{ DebuggerLogger::DEBUGGER_LOGGER_MAX_MESSAGE_LEN }>;

/// Writes records to the kernel debugger with `DbgPrintEx`.
pub(crate) struct DebuggerLogger {
    options: DebugLoggerOptions,
}

impl DebuggerLogger {
    pub const DEBUGGER_LOGGER_MAX_MESSAGE_LEN: usize = 512;

    pub(crate) const fn new(options: DebugLoggerOptions) -> Self {
        Self { options }
    }

    /// The line printed for `entry`, newline included, or `Err` if it does not fit.
    fn line(entry: &Entry<'_>) -> Result<Line, fmt::Error> {
        let mut line = Line::new();
        writeln!(line, "{entry}")?;
        Ok(line)
    }

    #[cfg(test)]
    pub(crate) const fn options(&self) -> DebugLoggerOptions {
        self.options
    }
}

impl Sink for DebuggerLogger {
    fn max_level(&self) -> Option<LevelFilter> {
        self.options.max_level
    }

    fn write(&self, entry: &Entry<'_>) {
        let component_id = self.options.component.raw();
        let dbgprint_level = self.options.dbgprint_level.raw();

        if let Ok(line) = Self::line(entry) {
            let len = i32::try_from(line.len()).unwrap_or(i32::MAX);
            // SAFETY: a literal format, so a `%` in the line is printed as is. `%.*s` reads
            // `len` bytes from `line`, which holds that many.
            unsafe {
                DbgPrintEx(
                    component_id,
                    dbgprint_level,
                    c"%.*s".as_ptr().cast(),
                    len,
                    line.as_ptr(),
                );
            }
        } else {
            // SAFETY: a literal format with no conversion specification, so DbgPrintEx
            // reads no argument.
            unsafe {
                DbgPrintEx(
                    component_id,
                    dbgprint_level,
                    c"Failed to format log record!\n".as_ptr().cast(),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use log::{Level, Record};

    /// 1970-01-01 00:00:00 UTC, in system time.
    const UNIX_EPOCH: i64 = 116_444_736_000_000_000;

    /// The line printed for an info record with `message`.
    fn line_for(message: &str) -> Result<Line, fmt::Error> {
        DebuggerLogger::line(&Entry::new(
            &Record::builder()
                .args(format_args!("{message}"))
                .level(Level::Info)
                .target("test_target")
                .build(),
            1234,
            5678,
            UNIX_EPOCH,
        ))
    }

    #[test]
    fn the_options_start_at_the_defaults_and_follow_the_setters() {
        let defaults = DebugLoggerOptions::new();
        assert_eq!(DebugLoggerOptions::default(), defaults);
        assert_eq!(
            (
                defaults.max_level,
                defaults.component,
                defaults.dbgprint_level
            ),
            (None, DbgPrintComponent::IHV_DRIVER, DbgPrintLevel::ERROR)
        );

        let options = DebugLoggerOptions::new()
            .with_max_level(LevelFilter::Debug)
            .with_component(DbgPrintComponent::IHV_BUS)
            .with_dbgprint_level(DbgPrintLevel::TRACE);
        assert_eq!(
            (options.max_level, options.component, options.dbgprint_level),
            (
                Some(LevelFilter::Debug),
                DbgPrintComponent::IHV_BUS,
                DbgPrintLevel::TRACE
            )
        );
    }

    #[test]
    fn the_debugger_takes_the_level_of_its_options() {
        let inherits = DebuggerLogger::new(DebugLoggerOptions::new());
        assert_eq!(inherits.max_level(), None);

        let own = DebuggerLogger::new(DebugLoggerOptions::new().with_max_level(LevelFilter::Warn));
        assert_eq!(own.max_level(), Some(LevelFilter::Warn));
    }

    #[test]
    fn a_line_is_the_entry_with_a_newline() {
        assert_eq!(
            line_for("Test log message").unwrap().as_str(),
            "1970-01-01 00:00:00 INFO  [1234|5678] [test_target] Test log message\n"
        );
    }

    #[test]
    fn a_line_longer_than_dbgprintex_prints_is_refused() {
        let long = [b'x'; DebuggerLogger::DEBUGGER_LOGGER_MAX_MESSAGE_LEN];
        assert!(line_for(core::str::from_utf8(&long).unwrap()).is_err());
    }
}
