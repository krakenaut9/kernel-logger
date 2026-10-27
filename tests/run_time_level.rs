//! `log::set_max_level` changes the installed logger's level while it runs, up as well as
//! down.
//!
//! A file of its own, so that it runs in a process of its own: `log` takes one logger
//! per process. It never logs, since that would run kernel functions.

// Linking `set_logger` imports kernel functions from ntoskrnl.exe, which a 64-bit test
// process can load. WOW64 has no 32-bit ntoskrnl.exe, so on x86 the process would not
// start.
#![cfg(not(target_arch = "x86"))]

use kernel_logger::kernel_logger::{KernelLogger, KernelLoggerBuilder};
use log::{Level, LevelFilter, Metadata};

/// The metadata of a record at `level`.
fn at(level: Level) -> Metadata<'static> {
    Metadata::builder().level(level).build()
}

#[test]
fn log_set_max_level_changes_the_loggers_level() {
    KernelLogger::set_logger(KernelLoggerBuilder::new().with_debug_logger().build()).unwrap();

    // The default level.
    assert_eq!(log::max_level(), LevelFilter::Info);
    assert!(!log::logger().enabled(&at(Level::Debug)));

    log::set_max_level(LevelFilter::Debug);

    assert!(log::logger().enabled(&at(Level::Debug)));

    log::set_max_level(LevelFilter::Off);

    assert!(!log::logger().enabled(&at(Level::Error)));
}
