#![allow(static_mut_refs)]

extern crate alloc;

use bitflags::bitflags;
use core::fmt::Write;
use heapless::String;
use log::{Level, Log, Metadata, Record, SetLoggerError};
use windows_sys::Wdk::System::SystemServices::DbgPrintEx;

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

trait Logger {
    fn log(&self, record: &Record);

    fn write_format_record(
        record: &Record,
        message: &mut impl Write,
    ) -> Result<(), core::fmt::Error> {
        core::write!(
            message,
            "{:<5} [{}] {}\n\0",
            record.level().as_str(),
            record.target(),
            record.args()
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
            if let Some(debugger_logger) = &self.debugger_logger {
                debugger_logger.log(record);
            }
        }
    }

    fn flush(&self) {}
}

struct DebuggerLogger;

impl DebuggerLogger {
    const DEBUGGER_LOGGER_MAX_MESSAGE_LEN: usize = 256;
}

impl Logger for DebuggerLogger {
    fn log(&self, record: &Record) {
        let mut message: String<{ DebuggerLogger::DEBUGGER_LOGGER_MAX_MESSAGE_LEN }> =
            String::new();

        if let Err(_err) = Self::write_format_record(record, &mut message) {
            unsafe { DbgPrintEx(0, 0, b"Failed to format log record!\0\n".as_ptr()) };
        } else {
            unsafe { DbgPrintEx(0, 0, message.as_ptr().cast()) };
        }
    }
}
