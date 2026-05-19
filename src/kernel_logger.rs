#![allow(static_mut_refs)]

use crate::{
    loggers::debug::DebuggerLogger,
    time_util::{current_time, windows_time_to_offset_datetime},
};
use bitflags::bitflags;
use core::fmt::{self, Write};
use log::{Level, Log, Metadata, Record, SetLoggerError};
use time::OffsetDateTime;
use windows_sys::Wdk::System::SystemServices::{PsGetCurrentProcessId, PsGetCurrentThreadId};

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct Loggers: u32 {
        const DEBUGGER = 0b0000_0001;
    }
}

static mut LOGGER: Option<KernelLogger> = None;

pub struct KernelLoggerBuilder {
    loggers: Loggers,
    log_level: Level,
}

impl KernelLoggerBuilder {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with_log_level(mut self, level: Level) -> Self {
        self.log_level = level;
        self
    }

    #[must_use]
    pub fn with_debug_logger(mut self) -> Self {
        self.loggers.insert(Loggers::DEBUGGER);
        self
    }

    #[must_use]
    pub fn build(self) -> KernelLogger {
        let mut kernel_logger = KernelLogger {
            log_level: self.log_level,
            ..Default::default()
        };

        if self.loggers.contains(Loggers::DEBUGGER) {
            kernel_logger.debugger_logger = Some(DebuggerLogger);
        }

        kernel_logger
    }
}

impl Default for KernelLoggerBuilder {
    fn default() -> Self {
        Self {
            loggers: Loggers::default(),
            log_level: Level::Trace,
        }
    }
}

pub(crate) trait Logger {
    fn log(&self, record: &Record, pid: u32, tid: u32, timestamp: Option<OffsetDateTime>);

    fn write_format_record(
        record: &Record,
        pid: u32,
        tid: u32,
        timestamp: Option<OffsetDateTime>,
        message: &mut impl Write,
    ) -> Result<(), core::fmt::Error> {
        let timestamp = timestamp.unwrap_or(OffsetDateTime::UNIX_EPOCH);

        Self::write_offset_datetime(message, timestamp)?;

        write!(
            message,
            " {:<5} [{pid}|{tid}] [{}] {}\n\0",
            record.level().as_str(),
            record.target(),
            record.args()
        )
    }

    #[inline]
    fn write_offset_datetime<W: Write>(out: &mut W, dt: OffsetDateTime) -> fmt::Result {
        let date = dt.date();
        let time = dt.time();

        write!(
            out,
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
            date.year(),
            u8::from(date.month()),
            date.day(),
            time.hour(),
            time.minute(),
            time.second()
        )
    }
}

pub struct KernelLogger {
    debugger_logger: Option<DebuggerLogger>,
    log_level: Level,
}

impl KernelLogger {
    pub fn set_logger(logger: Self) -> Result<(), SetLoggerError> {
        unsafe { LOGGER = Some(logger) };
        let logger = unsafe { LOGGER.as_ref().expect("Logger was created") };
        log::set_max_level(logger.log_level.to_level_filter());
        log::set_logger(logger)
    }
}

impl Default for KernelLogger {
    fn default() -> Self {
        Self {
            debugger_logger: None,
            log_level: Level::Trace,
        }
    }
}

impl Log for KernelLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= self.log_level
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            let pid = unsafe { PsGetCurrentProcessId() as u32 };
            let tid = unsafe { PsGetCurrentThreadId() as u32 };
            let timestamp = windows_time_to_offset_datetime(current_time()).ok();
            if let Some(debugger_logger) = &self.debugger_logger {
                debugger_logger.log(record, pid, tid, timestamp);
            }
        }
    }

    fn flush(&self) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use heapless::String;
    use log::{Level, Record};

    #[test]
    fn test_format_log_record() {
        let record = Record::builder()
            .args(format_args!("Test log message"))
            .level(Level::Info)
            .target("test_target")
            .build();

        let mut message = String::<{ DebuggerLogger::DEBUGGER_LOGGER_MAX_MESSAGE_LEN }>::new();
        <DebuggerLogger as Logger>::write_format_record(
            &record,
            1234,
            5678,
            Some(OffsetDateTime::UNIX_EPOCH),
            &mut message,
        )
        .unwrap();

        let expected_message =
            "1970-01-01 00:00:00 INFO  [1234|5678] [test_target] Test log message\n\0";
        assert_eq!(message.as_str(), expected_message);
    }
}
