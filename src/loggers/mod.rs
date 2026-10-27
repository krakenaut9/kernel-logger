pub mod dbgprint;
pub mod debug;

use crate::time_util::Timestamp;
use core::fmt;
use log::{LevelFilter, Record};

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
/// call it again. A sink that cannot work at every IRQL checks [`Entry::irql`] and skips
/// what it cannot handle, as the kernel debugger sink does above DIRQL.
///
/// ```
/// use core::sync::atomic::{AtomicUsize, Ordering};
/// use kernel_logger::kernel_logger::{Entry, KernelLoggerBuilder, Sink};
/// use log::LevelFilter;
///
/// /// Counts the errors.
/// struct ErrorCounter(AtomicUsize);
///
/// impl Sink for ErrorCounter {
///     fn max_level(&self) -> LevelFilter {
///         LevelFilter::Error
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
    /// The most verbose records this sink takes: [`LevelFilter::Trace`], the default, takes
    /// every record the logger's level
    /// ([`with_log_level`](crate::kernel_logger::KernelLoggerBuilder::with_log_level)) lets
    /// through, and [`LevelFilter::Off`] none. No sink takes a record above the logger's
    /// level, whatever this returns.
    ///
    /// It is read for every record within the logger's level, so a sink may change it at any
    /// time.
    fn max_level(&self) -> LevelFilter {
        LevelFilter::Trace
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
/// Its [`Display`](fmt::Display) form is the record as one line of text, without a newline,
/// starting with the time in UTC to the microsecond:
///
/// ```text
/// 2026-05-19 10:29:12.345678Z INFO  [4|1234] [my_driver::driver] Driver started
/// ```
///
/// A time outside the years 0 to 9999 shows as `????-??-?? ??:??:??.??????Z`.
#[derive(Debug, Clone, Copy)]
pub struct Entry<'a> {
    record: &'a Record<'a>,
    process_id: usize,
    thread_id: usize,
    irql: u8,
    system_time: i64,
}

impl<'a> Entry<'a> {
    pub(crate) const fn new(
        record: &'a Record<'a>,
        process_id: usize,
        thread_id: usize,
        irql: u8,
        system_time: i64,
    ) -> Self {
        Self {
            record,
            process_id,
            thread_id,
            irql,
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

    /// The IRQL the record was logged at, as `KeGetCurrentIrql` returns it.
    ///
    /// Above `DISPATCH_LEVEL`, the levels depend on the architecture: `HIGH_LEVEL`, for one,
    /// is 31 on x86 and 15 on x64 and ARM64. So compare it with values made for the target,
    /// such as those wdk-sys generates from the WDK's headers. windows-sys 0.61 gives
    /// `CLOCK_LEVEL`, `PROFILE_LEVEL`, `IPI_LEVEL`, `POWER_LEVEL` and `HIGH_LEVEL` their x86
    /// values on every architecture.
    #[must_use]
    pub const fn irql(&self) -> u8 {
        self.irql
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
        f.write_str(Timestamp::new(self.system_time).as_str())?;

        write!(
            f,
            " {:<5} [{}|{}] [{}] {}",
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
    use core::{fmt::Write, ptr};
    use heapless::String;
    use log::Level;

    /// 1970-01-01 00:00:00 UTC, in system time.
    const UNIX_EPOCH: i64 = 116_444_736_000_000_000;
    /// The IRQL DPCs run at.
    const DISPATCH_LEVEL: u8 = 2;

    #[test]
    fn a_sinks_level_is_trace_unless_it_sets_one() {
        /// A sink that keeps the default level.
        struct Plain;

        impl Sink for Plain {
            fn write(&self, _entry: &Entry<'_>) {}
        }

        assert_eq!(Plain.max_level(), LevelFilter::Trace);
    }

    #[test]
    fn an_entry_returns_what_the_logger_read() {
        let record = Record::builder().build();
        let entry = Entry::new(&record, 1234, 5678, DISPATCH_LEVEL, UNIX_EPOCH);

        assert!(ptr::eq(entry.record(), &raw const record));
        assert_eq!(
            (
                entry.process_id(),
                entry.thread_id(),
                entry.irql(),
                entry.system_time()
            ),
            (1234, 5678, DISPATCH_LEVEL, UNIX_EPOCH)
        );
    }

    #[test]
    fn an_entry_displays_as_the_log_line() {
        let record = Record::builder()
            .args(format_args!("Test log message"))
            .level(Level::Info)
            .target("test_target")
            .build();
        let entry = Entry::new(&record, 1234, 5678, DISPATCH_LEVEL, UNIX_EPOCH);

        let mut line = String::<512>::new();
        write!(line, "{entry}").unwrap();

        assert_eq!(
            line.as_str(),
            "1970-01-01 00:00:00.000000Z INFO  [1234|5678] [test_target] Test log message"
        );
    }
}
