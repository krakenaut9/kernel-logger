//! A driver's own sinks are enough for a logger, without the debugger, and their levels
//! count for `log`'s gate.
//!
//! A file of its own, so that it runs in a process of its own: `log` takes one logger
//! per process. It never logs, since that would run kernel functions.

// Linking `set_logger` imports kernel functions from ntoskrnl.exe, which a 64-bit test
// process can load. WOW64 has no 32-bit ntoskrnl.exe, so on x86 the process would not
// start.
#![cfg(not(target_arch = "x86"))]

use kernel_logger::kernel_logger::{Entry, KernelLogger, KernelLoggerBuilder, Sink};
use log::{Level, LevelFilter, Metadata};

/// A sink that drops what it gets, with a level of its own or none.
struct Quiet(Option<LevelFilter>);

impl Sink for Quiet {
    fn max_level(&self) -> Option<LevelFilter> {
        self.0
    }

    fn write(&self, _entry: &Entry<'_>) {}
}

static UP_TO_DEBUG: Quiet = Quiet(Some(LevelFilter::Debug));
static AT_LOGGERS_LEVEL: Quiet = Quiet(None);
static SINKS: [&dyn Sink; 2] = [&UP_TO_DEBUG, &AT_LOGGERS_LEVEL];

#[test]
fn the_drivers_own_sinks_set_the_gate() {
    KernelLogger::set_logger(
        KernelLoggerBuilder::new()
            .with_sinks(&SINKS)
            .with_log_level(Level::Info)
            .build(),
    )
    .unwrap();

    assert_eq!(log::max_level(), LevelFilter::Debug);
    assert!(log::logger().enabled(&Metadata::builder().level(Level::Debug).build()));
    assert!(!log::logger().enabled(&Metadata::builder().level(Level::Trace).build()));
}
