//! A logger built without any sink, not even the debugger, logs nothing, whatever its
//! level.
//!
//! A file of its own, so that it runs in a process of its own: `log` takes one logger
//! per process. It never logs, since that would run kernel functions.

// Linking `set_logger` imports kernel functions from ntoskrnl.exe, which a 64-bit test
// process can load. WOW64 has no 32-bit ntoskrnl.exe, so on x86 the process would not
// start.
#![cfg(not(target_arch = "x86"))]

use kernel_logger::kernel_logger::{KernelLogger, KernelLoggerBuilder};
use log::{Level, LevelFilter, Metadata};

#[test]
fn a_logger_without_sinks_logs_nothing() {
    KernelLogger::set_logger(
        KernelLoggerBuilder::new()
            .with_log_level(LevelFilter::Trace)
            .build(),
    )
    .unwrap();

    assert_eq!(log::max_level(), LevelFilter::Trace);
    assert!(!log::logger().enabled(&Metadata::builder().level(Level::Error).build()));
}
