//! 016: Fallow's window on macOS (research M-1 to M-13): AppKit on the main thread, an `NSWindow`
//! Fallow makes itself and WebKit's `WKWebView` through `wry`. The decisions it is made of
//! (`nav`, `state`, `quit`) stay plain and tested on every platform.

pub mod alert;
pub mod app;
pub mod frame;
pub mod host;
pub mod menu;
pub mod view;
