use crate::kernel_logger::Logger;
use heapless::String;
use log::Record;
use time::OffsetDateTime;
use windows_sys::Wdk::System::SystemServices::DbgPrintEx;

pub(crate) struct DebuggerLogger;

impl DebuggerLogger {
    pub const DEBUGGER_LOGGER_MAX_MESSAGE_LEN: usize = 512;
}

impl Logger for DebuggerLogger {
    fn log(&self, record: &Record, pid: u32, tid: u32, timestamp: Option<OffsetDateTime>) {
        let mut message: String<{ DebuggerLogger::DEBUGGER_LOGGER_MAX_MESSAGE_LEN }> =
            String::new();

        if let Err(_err) = Self::write_format_record(record, pid, tid, timestamp, &mut message) {
            unsafe { DbgPrintEx(0, 0, c"Failed to format log record!\n".as_ptr().cast()) };
        } else {
            unsafe { DbgPrintEx(0, 0, message.as_ptr().cast()) };
        }
    }
}
