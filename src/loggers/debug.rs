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

/// The size of the buffer `DbgPrintEx` formats each call into: it transmits at most this
/// many bytes per call.
const DBGPRINTEX_BUFFER_SIZE: usize = 512;
/// The most a line may take. Windows does not document whether the buffer's last byte can
/// hold text or is kept for the NUL that ends it, and the `ReactOS` implementation keeps it
/// for the NUL. A line that lost its last byte would lose its newline and run the next
/// record onto it, so that byte is left free.
const LINE_CAPACITY: usize = DBGPRINTEX_BUFFER_SIZE - 1;
/// Ends every line: `DbgPrintEx` adds no newline of its own.
const NEWLINE: &str = "\n";
/// Ends the text of a line that was cut.
const CUT_MARKER: &str = "...";
/// The room for text: what is left once the marker and the newline are set aside, so that
/// both always fit after it.
const TEXT_CAPACITY: usize = LINE_CAPACITY - CUT_MARKER.len() - NEWLINE.len();

/// One line for `DbgPrintEx`. Text that does not fit is cut at a character boundary, and the
/// line then ends with [`CUT_MARKER`].
struct Line {
    text: String<LINE_CAPACITY>,
    /// Whether the text was cut: where the next text did not fit, or where a value failed to
    /// format.
    cut: bool,
}

impl Line {
    /// The line for `entry`: its text, the marker if the text was cut, and the newline.
    fn new(entry: &Entry<'_>) -> Self {
        let mut line = Self {
            text: String::new(),
            cut: false,
        };
        // Formatting stops at the first error: the one `write_str` returns when it cuts the
        // text, or one from a value that failed to format. Either way, the rest is missing.
        if write!(line, "{entry}").is_err() {
            line.cut = true;
        }
        // TEXT_CAPACITY leaves room for both, so neither push fails.
        if line.cut {
            let _ = line.text.push_str(CUT_MARKER);
        }
        let _ = line.text.push_str(NEWLINE);
        line
    }
}

impl Write for Line {
    /// Appends `text` if it all fits. Otherwise appends the part that fits, cut at a
    /// character boundary, and returns an error, since `write_str` may succeed only when it
    /// wrote all of `text`. The error stops the formatting, so the rest of the record is not
    /// formatted only to be dropped, and `cut` tells `new` what happened. After a cut, it
    /// appends nothing more.
    fn write_str(&mut self, text: &str) -> fmt::Result {
        if self.cut {
            return Err(fmt::Error);
        }
        let room = TEXT_CAPACITY.saturating_sub(self.text.len());
        if text.len() <= room {
            return self.text.push_str(text).map_err(|_| fmt::Error);
        }
        // The last character boundary within the room. 0 is always one.
        let fits = (0..=room)
            .rev()
            .find(|&index| text.is_char_boundary(index))
            .unwrap_or_default();
        let _ = self.text.push_str(text.get(..fits).unwrap_or_default());
        self.cut = true;
        Err(fmt::Error)
    }
}

/// Writes records to the kernel debugger with `DbgPrintEx`.
pub(crate) struct DebuggerLogger {
    options: DebugLoggerOptions,
}

impl DebuggerLogger {
    pub(crate) const fn new(options: DebugLoggerOptions) -> Self {
        Self { options }
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
        let line = Line::new(entry);
        let len = i32::try_from(line.text.len()).unwrap_or(i32::MAX);
        // SAFETY: a literal format, so a `%` in the line is printed as is. `%.*s` reads `len`
        // bytes from the line, which holds that many.
        unsafe {
            DbgPrintEx(
                self.options.component.raw(),
                self.options.dbgprint_level.raw(),
                c"%.*s".as_ptr().cast(),
                len,
                line.text.as_ptr(),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::{cell::Cell, str};
    use log::{Level, Record};

    /// 1970-01-01 00:00:00 UTC, in system time.
    const UNIX_EPOCH: i64 = 116_444_736_000_000_000;

    /// Enough `x` for any message below.
    static X_BYTES: [u8; 10 * 1024] = [b'x'; 10 * 1024];

    /// `len` times `x`.
    fn xs(len: usize) -> &'static str {
        str::from_utf8(&X_BYTES[..len]).unwrap()
    }

    /// The line for an info record with `message`.
    fn line_for(message: fmt::Arguments<'_>) -> Line {
        Line::new(&Entry::new(
            &Record::builder()
                .args(message)
                .level(Level::Info)
                .target("test_target")
                .build(),
            1234,
            5678,
            UNIX_EPOCH,
        ))
    }

    /// How long a line is before its message.
    fn prefix_len() -> usize {
        line_for(format_args!("")).text.len() - NEWLINE.len()
    }

    /// What every line must be: no longer than `LINE_CAPACITY`, and ending with the newline.
    fn assert_well_formed(line: &Line) {
        assert!(line.text.len() <= LINE_CAPACITY);
        assert!(line.text.ends_with(NEWLINE));
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
    fn a_short_line_is_written_whole() {
        let line = line_for(format_args!("Test log message"));
        assert_eq!(
            line.text,
            "1970-01-01 00:00:00 INFO  [1234|5678] [test_target] Test log message\n"
        );
        assert!(!line.cut);
    }

    #[test]
    fn percent_signs_stay_as_they_are() {
        let line = line_for(format_args!("100% %s %n %ws"));
        assert!(line.text.ends_with("[test_target] 100% %s %n %ws\n"));
    }

    #[test]
    fn a_line_that_fits_exactly_is_not_cut() {
        let line = line_for(format_args!("{}", xs(TEXT_CAPACITY - prefix_len())));
        assert_well_formed(&line);
        assert!(!line.cut);
        assert_eq!(line.text.len(), TEXT_CAPACITY + NEWLINE.len());
        assert!(line.text.ends_with("x\n"));
    }

    #[test]
    fn one_byte_more_is_cut_and_marked() {
        let line = line_for(format_args!("{}", xs(TEXT_CAPACITY - prefix_len() + 1)));
        assert_well_formed(&line);
        assert!(line.cut);
        assert_eq!(line.text.len(), LINE_CAPACITY);
        assert!(line.text.ends_with("x...\n"));
    }

    #[test]
    fn a_character_across_the_limit_is_not_split() {
        for character in ['é', '€', '😀'] {
            let width = character.len_utf8();
            // Too little room drops the character whole, however little is missing.
            for room in 1..width {
                let line = line_for(format_args!(
                    "{}{character}",
                    xs(TEXT_CAPACITY - prefix_len() - room)
                ));
                assert_well_formed(&line);
                assert!(line.cut);
                assert_eq!(line.text.len(), LINE_CAPACITY - room);
                assert!(line.text.ends_with("x...\n"));
            }
            // Just enough room keeps it.
            let line = line_for(format_args!(
                "{}{character}",
                xs(TEXT_CAPACITY - prefix_len() - width)
            ));
            assert_well_formed(&line);
            assert!(!line.cut);
            assert!(
                line.text
                    .strip_suffix(NEWLINE)
                    .is_some_and(|text| text.ends_with(character))
            );
        }
    }

    #[test]
    fn formatting_stops_at_the_cut() {
        /// Counts how often it is formatted.
        struct Counted<'a>(&'a Cell<usize>);

        impl fmt::Display for Counted<'_> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.set(self.0.get() + 1);
                f.write_str("z")
            }
        }

        let formatted = Cell::new(0);
        let line = line_for(format_args!("{}{}", xs(10 * 1024), Counted(&formatted)));
        assert!(line.cut);
        // The cut came first, so the value after it was never formatted.
        assert_eq!(formatted.get(), 0);
    }

    #[test]
    fn a_10_kb_message_makes_one_line() {
        let line = line_for(format_args!("{}", xs(10 * 1024)));
        assert_well_formed(&line);
        assert!(line.cut);
        assert!(line.text.ends_with("x...\n"));
    }

    #[test]
    fn a_value_that_fails_to_format_ends_the_line_with_the_marker() {
        /// Writes part of itself, then fails.
        struct Fails;

        impl fmt::Display for Fails {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("half")?;
                Err(fmt::Error)
            }
        }

        let line = line_for(format_args!("before {Fails} after"));
        assert_well_formed(&line);
        assert!(line.cut);
        assert!(line.text.ends_with("[test_target] before half...\n"));
    }

    #[test]
    fn nothing_is_written_after_the_cut() {
        /// Writes "é", which does not fit, then ignores the error and goes on.
        struct IgnoresErrors;

        impl fmt::Display for IgnoresErrors {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                let _ = f.write_str("é");
                f.write_str("a")
            }
        }

        // One byte is left: too few for "é", enough for "a".
        let line = line_for(format_args!(
            "{}{}",
            xs(TEXT_CAPACITY - prefix_len() - 1),
            IgnoresErrors
        ));
        assert_well_formed(&line);
        assert!(line.text.ends_with("x...\n"));
    }
}
