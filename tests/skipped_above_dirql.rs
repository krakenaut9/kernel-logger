//! `KernelLogger::debug_logger_skipped_above_dirql` reads the count of the debugger sink in
//! use, through the installed logger.
//!
//! A file of its own, so that it runs in a process of its own: `log` takes one logger
//! per process. It never logs, since that would run kernel functions.

// Linking `set_logger` imports kernel functions from ntoskrnl.exe, which a 64-bit test
// process can load. WOW64 has no 32-bit ntoskrnl.exe, so on x86 the process would not
// start.
#![cfg(not(target_arch = "x86"))]

use kernel_logger::kernel_logger::{KernelLogger, KernelLoggerBuilder};

#[test]
fn nothing_is_counted_before_a_record_is_skipped() {
    // No logger installed yet.
    assert_eq!(KernelLogger::debug_logger_skipped_above_dirql(), 0);

    KernelLogger::set_logger(KernelLoggerBuilder::new().with_debug_logger().build()).unwrap();
    assert_eq!(KernelLogger::debug_logger_skipped_above_dirql(), 0);
}
