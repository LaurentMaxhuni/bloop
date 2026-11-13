//! Linux implementations of the `platform` functions. Pure helpers compile on every OS under
//! `cfg(test)` so their tests run on Windows too; code that talks to the desktop is Linux-only.

#[cfg(target_os = "linux")]
pub mod icon;
#[cfg(target_os = "linux")]
pub mod instance;
pub mod monitors;
pub mod session;
pub mod shape;
#[cfg(target_os = "linux")]
pub mod x11;

#[cfg(target_os = "linux")]
pub use session::init_backend;
#[allow(unused_imports)]
pub use session::{Session, child_env, child_env_tokio, session};
