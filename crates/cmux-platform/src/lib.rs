//! Operating-system services shared by the desktop and command-line client.
//!
//! Linux is currently the only supported backend. This library owns native
//! policy without depending on GTK or workspace state.

#![deny(missing_docs, unsafe_op_in_unsafe_fn)]

#[cfg(not(target_os = "linux"))]
compile_error!("cmux-platform currently supports Linux only");

pub mod discovery;
pub mod filesystem;
pub mod installation;
pub mod local_socket;
pub mod notification;
pub mod paths;
pub mod peer;
pub mod process;

#[cfg(feature = "gtk")]
pub mod window;

#[cfg(feature = "gtk")]
pub mod opengl;

pub mod terminal;
