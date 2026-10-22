//! A sink's own level counts for `log`'s gate, which lets through the most verbose records
//! any sink takes.
//!
//! A file of its own, so that it runs in a process of its own: `log` takes one logger
//! per process. It never logs, since that would run kernel functions.

// Linking `set_logger` imports kernel functions from ntoskrnl.exe, which a 64-bit test
// process can load. WOW64 has no 32-bit ntoskrnl.exe, so on x86 the process would not
// start.
#![cfg(not(target_arch = "x86"))]

use kernel_logger::kernel_logger::{DebugLoggerOptions, KernelLogger, KernelLoggerBuilder};
use log::{Level, LevelFilter, Metadata};

#[test]
fn the_debuggers_own_level_sets_the_gate() {
    KernelLogger::set_logger(
        KernelLoggerBuilder::new()
            .with_debug_logger_options(DebugLoggerOptions::new().with_max_level(LevelFilter::Debug))
            .with_log_level(Level::Info)
            .build(),
    )
    .unwrap();

    assert_eq!(log::max_level(), LevelFilter::Debug);
    assert!(log::logger().enabled(&Metadata::builder().level(Level::Debug).build()));
    assert!(!log::logger().enabled(&Metadata::builder().level(Level::Trace).build()));
}
