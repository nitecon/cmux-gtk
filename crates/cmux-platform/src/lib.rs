//! Operating-system services shared by the desktop and command-line client.
//!
//! Linux and the experimental Windows backend own native policy without
//! depending on GTK or workspace state.

#![deny(missing_docs, unsafe_op_in_unsafe_fn)]

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
compile_error!("cmux-platform supports Linux and Windows");

#[cfg(windows)]
mod windows;

pub mod discovery;
#[cfg_attr(windows, path = "windows/filesystem.rs")]
pub mod filesystem;
pub mod installation;
#[cfg_attr(windows, path = "windows/listeners.rs")]
pub mod listeners;
#[cfg_attr(windows, path = "windows/local_socket.rs")]
pub mod local_socket;
#[cfg_attr(windows, path = "windows/notification.rs")]
pub mod notification;
pub mod paths;
#[cfg_attr(windows, path = "windows/peer.rs")]
pub mod peer;
#[cfg_attr(windows, path = "windows/process.rs")]
pub mod process;

#[cfg(feature = "gtk")]
pub mod window;

#[cfg(feature = "gtk")]
pub mod opengl;

#[cfg_attr(windows, path = "windows/terminal.rs")]
pub mod terminal;
