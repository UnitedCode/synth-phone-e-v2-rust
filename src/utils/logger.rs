//! # Embedded Logging Configuration
//!
//! This module is designed for ARM Cortex-M microcontrollers, providing a flexible logging
//! framework that supports various output mechanisms, such as ITM (Instrumentation Trace Macrocell),
//! RTT (Real-Time Transfer), and Semihosting. The logging backend to be used can be selected
//! at compile-time based on the project's configuration and specific needs for debugging or
//! outputting information.
//!
//! ## Features
//! - `log-itm`: Enables logging over ITM, suitable for real-time applications where minimal
//!   interference with the program's execution is desired.
//! - `log-rtt`: Utilizes Segger's RTT for logging, offering a balance between performance and
//!   ease of use, especially useful for applications that debug through a Segger J-Link.
//! - `log-semihosting`: Uses the semihosting feature, allowing logs to be output directly to
//!   the debugging computer's console. Best used in scenarios where performance is not critical.
//!
//! Each logging backend comes with its initialization function that prepares the logging system
//! based on the selected method. The configuration makes use of conditional compilation controlled
//! by feature flags to include only the relevant code for the chosen logging method.
//!
//! ## Usage
//!
//! To use this logging configuration in your project, you must first decide on the logging backend
//! that suits your needs and enable the corresponding feature in your `Cargo.toml`. Then, ensure
//! to call the `init` function early in your program's startup sequence to initialize the logging
//! system.
//!
//! Example for enabling ITM logging in `Cargo.toml`:
//! ```toml
//! [dependencies.your-package]
//! features = ["log-itm"]
//! ```
//!
//! Initializing the logger in your application:
//! ```rust
//! fn main() {
//!     logger::init();
//!     // Now you can use log macros, e.g., info!("This is a log message.");
//! }
//! ```
//!
//! ## Panics
//! - Initialization functions for each logging method may panic if the logging system cannot be
//!   set up, such as failure to acquire peripherals for ITM or Semihosting output channels.
//!
//! ## Safety Considerations
//! - When using `unsafe` blocks, particularly for logging destinations that interact directly
//!   with hardware peripherals, care must be taken to ensure that the code does not interfere with
//!   the correct operation of other parts of the system or violate Rust's safety guarantees.
//!
//! ## Dependencies
//! This module relies on several external crates, including `log`, `cortex-m-log`, `rtt-target`,
//! `panic-itm`, `panic-rtt-target`, `panic-semihosting`, and `panic-halt`, to provide its
//! functionality. These dependencies should be included in your project's `Cargo.toml` file.
//!
//! ## Note
//! This configuration and initialization module is an essential part of setting up a project for
//! embedded development with logging capabilities. It abstracts away much of the complexity
//! involved in setting up different logging backends, allowing developers to focus on building
//! their application.


#![cfg_attr(feature = "log-itm", allow(unsafe_code))]

cfg_if::cfg_if! {
    if #[cfg(any(feature = "log-itm"))] {
        use panic_itm as _;

        use lazy_static::lazy_static;
        use log::LevelFilter;

        pub use cortex_m_log::log::Logger;

        use cortex_m_log::{
            destination::Itm as ItmDest,
            printer::itm::InterruptSync,
            modes::InterruptFree,
            printer::itm::ItmSync
        };

        lazy_static! {
            static ref LOGGER: Logger<ItmSync<InterruptFree>> = Logger {
                level: LevelFilter::Info,
                inner: unsafe {
                    InterruptSync::new(
                        ItmDest::new(cortex_m::Peripherals::steal().ITM)
                    )
                },
            };
        }

        pub fn init() {
            cortex_m_log::log::init(&LOGGER).unwrap();
        }

    }
    else if #[cfg(any(feature = "log-rtt"))] {
        use panic_rtt_target as _;

        use log::{Level, Metadata, Record, LevelFilter};
        use rtt_target::{rprintln, rtt_init_print};

        pub struct Logger {
            level: Level,
        }

        static LOGGER: Logger = Logger {
            level: Level::Info,
        };

        pub fn init() {
            rtt_init_print!();
            log::set_logger(&LOGGER).map(|()| log::set_max_level(LevelFilter::Info)).unwrap();
        }

        impl log::Log for Logger {
            fn enabled(&self, metadata: &Metadata) -> bool {
                metadata.level() <= self.level

            }

            fn log(&self, record: &Record) {
                rprintln!("{} - {}", record.level(), record.args());
            }

            fn flush(&self) {}
        }
    }
    else if #[cfg(any(feature = "log-semihost"))] {
        use panic_semihosting as _;

        use lazy_static::lazy_static;
        use log::LevelFilter;

        pub use cortex_m_log::log::Logger;
        use cortex_m_log::printer::semihosting;
        use cortex_m_log::printer::semihosting::Semihosting;
        use cortex_m_log::modes::InterruptOk;
        use cortex_m_semihosting::hio::HStdout;

        lazy_static! {
            static ref LOGGER: Logger<Semihosting<InterruptOk, HStdout>> = Logger {
                level: LevelFilter::Info,
                inner: semihosting::InterruptOk::<_>::stdout().expect("Get Semihosting stdout"),
            };
        }

        pub fn init() {
            cortex_m_log::log::init(&LOGGER).unwrap();
        }
    }
    else {
        use panic_halt as _;
        pub fn init() {}
    }
}
