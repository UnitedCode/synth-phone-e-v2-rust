#![no_std]
// Declare modules
pub mod config;
// pub mod audio;
pub mod hardware;
pub mod utils;
// pub mod alloc;

// Re-export commonly used items for convenience
// pub use config::*;
// pub use audio::*;
pub use hardware::*;
pub use utils::*;
// pub use alloc::*;
