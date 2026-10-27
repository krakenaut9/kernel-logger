//! `KernelLogger::set_logger` installs the logger once and refuses any later call.
//!
//! A file of its own, so that it runs in a process of its own: `log` takes one logger
//! per process. It never logs, since that would run kernel functions.

// Linking `set_logger` imports kernel functions from ntoskrnl.exe, which a 64-bit test
// process can load. WOW64 has no 32-bit ntoskrnl.exe, so on x86 the process would not
// start.
#![cfg(not(target_arch = "x86"))]

use kernel_logger::kernel_logger::{KernelLogger, KernelLoggerBuilder};
use log::{Level, LevelFilter, Metadata};

fn logger_at(level: LevelFilter) -> KernelLogger {
    KernelLoggerBuilder::new()
        .with_debug_logger()
        .with_log_level(level)
        .build()
}

#[test]
fn a_second_set_logger_fails_and_changes_nothing() {
    let debug = Metadata::builder().level(Level::Debug).build();

    KernelLogger::set_logger(logger_at(LevelFilter::Info)).unwrap();
    assert_eq!(log::max_level(), LevelFilter::Info);
    assert!(!log::logger().enabled(&debug));

    assert!(KernelLogger::set_logger(logger_at(LevelFilter::Trace)).is_err());
    assert_eq!(log::max_level(), LevelFilter::Info);
    assert!(!log::logger().enabled(&debug));
}
