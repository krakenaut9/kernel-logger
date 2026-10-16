use crate::kernel_logger::Logger;
use heapless::String;
use log::Record;
use time::OffsetDateTime;

// windows-sys imports DbgPrintEx from ntdll.dll, which a driver cannot load.
windows_link::link!("ntoskrnl.exe" "C" fn DbgPrintEx(componentid: u32, level: u32, format: *const u8, ...) -> u32);

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
            let len = i32::try_from(message.len()).unwrap_or(i32::MAX);
            // SAFETY: a literal format, so a `%` in the line is printed as is. `%.*s` reads
            // `len` bytes from `message`, which holds that many.
            unsafe { DbgPrintEx(0, 0, c"%.*s".as_ptr().cast(), len, message.as_ptr()) };
        }
    }
}
