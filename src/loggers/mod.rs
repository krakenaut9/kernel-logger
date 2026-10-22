pub mod dbgprint;
pub mod debug;

use crate::time_util::windows_time_to_offset_datetime;
use core::fmt;
use log::{LevelFilter, Record};
use time::OffsetDateTime;

/// A destination for records.
///
/// The kernel debugger is one, built in and enabled with
/// [`with_debug_logger`](crate::kernel_logger::KernelLoggerBuilder::with_debug_logger). A
/// driver gives sinks of its own to
/// [`with_sinks`](crate::kernel_logger::KernelLoggerBuilder::with_sinks).
///
/// The logger calls a sink where the record is logged: at any IRQL, in DPCs and ISRs, and
/// on several processors at once. So a sink must not allocate, wait, touch pageable code or
/// data, or take a lock that the code it interrupted may hold. Nor may it log, which would
/// call it again.
///
/// ```
/// use core::sync::atomic::{AtomicUsize, Ordering};
/// use kernel_logger::kernel_logger::{Entry, KernelLoggerBuilder, Sink};
/// use log::LevelFilter;
///
/// /// Counts the errors, whatever the logger's level.
/// struct ErrorCounter(AtomicUsize);
///
/// impl Sink for ErrorCounter {
///     fn max_level(&self) -> Option<LevelFilter> {
///         Some(LevelFilter::Error)
///     }
///
///     fn write(&self, _entry: &Entry<'_>) {
///         self.0.fetch_add(1, Ordering::Relaxed);
///     }
/// }
///
/// static ERRORS: ErrorCounter = ErrorCounter(AtomicUsize::new(0));
/// static SINKS: [&dyn Sink; 1] = [&ERRORS];
///
/// let logger = KernelLoggerBuilder::new()
///     .with_debug_logger()
///     .with_sinks(&SINKS)
///     .build();
/// // ...then `KernelLogger::set_logger(logger)`.
/// ```
pub trait Sink: Sync {
    /// The most verbose records this sink takes, or `None`, the default, for the logger's
    /// level ([`with_log_level`](crate::kernel_logger::KernelLoggerBuilder::with_log_level)).
    ///
    /// It is read for every record.
    /// [`set_logger`](crate::kernel_logger::KernelLogger::set_logger) also reads it once, to
    /// set `log`'s level to the most verbose any sink takes, and the `log` macros drop a
    /// record above that level before any sink sees it. So a sink may lower its level later,
    /// but cannot raise it above `log`'s level for records from the `log` macros.
    fn max_level(&self) -> Option<LevelFilter> {
        None
    }

    /// Writes `entry`, a record at [`max_level`](Self::max_level) or below.
    fn write(&self, entry: &Entry<'_>);

    /// Writes out whatever the sink still holds: called by `log::logger().flush()`. The
    /// default does nothing.
    fn flush(&self) {}
}

/// A record as a sink receives it: the record, and what the logger read from the kernel
/// when it was logged.
///
/// Its [`Display`](fmt::Display) form is the record as one line of text, without a newline:
///
/// ```text
/// 2026-05-19 10:29:12 INFO  [4|1234] [my_driver::driver] Driver started
/// ```
#[derive(Debug, Clone, Copy)]
pub struct Entry<'a> {
    record: &'a Record<'a>,
    process_id: usize,
    thread_id: usize,
    system_time: i64,
}

impl<'a> Entry<'a> {
    pub(crate) const fn new(
        record: &'a Record<'a>,
        process_id: usize,
        thread_id: usize,
        system_time: i64,
    ) -> Self {
        Self {
            record,
            process_id,
            thread_id,
            system_time,
        }
    }

    /// The record: its level, target, message and the rest.
    #[must_use]
    pub const fn record(&self) -> &'a Record<'a> {
        self.record
    }

    /// The ID of the process that owns the thread that logged the record, even while that
    /// thread is attached to another process. In a DPC or an ISR, the thread is whichever
    /// one it interrupted.
    #[must_use]
    pub const fn process_id(&self) -> usize {
        self.process_id
    }

    /// The ID of the thread that logged the record. In a DPC or an ISR, that is whichever
    /// thread it interrupted.
    #[must_use]
    pub const fn thread_id(&self) -> usize {
        self.thread_id
    }

    /// When the record was logged, as `KeQuerySystemTimePrecise` gives it: UTC, in
    /// 100-nanosecond intervals since 1601-01-01.
    #[must_use]
    pub const fn system_time(&self) -> i64 {
        self.system_time
    }
}

impl fmt::Display for Entry<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let timestamp =
            windows_time_to_offset_datetime(self.system_time).unwrap_or(OffsetDateTime::UNIX_EPOCH);
        let (date, time) = (timestamp.date(), timestamp.time());

        write!(
            f,
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02} {:<5} [{}|{}] [{}] {}",
            date.year(),
            u8::from(date.month()),
            date.day(),
            time.hour(),
            time.minute(),
            time.second(),
            self.record.level().as_str(),
            self.process_id,
            self.thread_id,
            self.record.target(),
            self.record.args()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::fmt::Write;
    use heapless::String;
    use log::Level;

    /// 1970-01-01 00:00:00 UTC, in system time.
    const UNIX_EPOCH: i64 = 116_444_736_000_000_000;

    #[test]
    fn an_entry_displays_as_the_log_line() {
        let record = Record::builder()
            .args(format_args!("Test log message"))
            .level(Level::Info)
            .target("test_target")
            .build();
        let entry = Entry::new(&record, 1234, 5678, UNIX_EPOCH);

        let mut line = String::<512>::new();
        write!(line, "{entry}").unwrap();

        assert_eq!(
            line.as_str(),
            "1970-01-01 00:00:00 INFO  [1234|5678] [test_target] Test log message"
        );
    }
}
