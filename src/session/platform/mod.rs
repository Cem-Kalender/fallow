//! Process control for sessions (002 research R-7): one small module per OS.

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub use unix::Group;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::Group;
