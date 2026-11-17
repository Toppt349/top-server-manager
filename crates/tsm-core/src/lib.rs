//! TOP Server Manager core engine.
//!
//! This crate contains every piece of infrastructure logic (projects, process
//! supervision, runtimes, databases, network diagnostics and hosting providers).
//! It has no dependency on Tauri so it can be driven by the desktop UI, the CLI
//! (`tsm-cli`) and the automated tests alike.

pub mod alerts;
pub mod apps;
pub mod domains;
pub mod backup;
pub mod config;
pub mod database;
pub mod engine;
pub mod error;
pub mod events;
pub mod exposure;
pub mod health;
pub mod hosting;
pub mod htaccess;
pub mod i18n;
pub mod launcher;
pub mod logs;
pub mod mailcatch;
pub mod metrics;
pub mod model;
pub mod network;
pub mod paths;
pub mod pgserver;
pub mod players;
pub mod ports;
pub mod process;
pub mod project;
pub mod redis;
pub mod runtime;
pub mod security;
pub mod stacks;
pub mod supervisor;
pub mod templates;
pub mod tools;
pub mod util;
#[cfg(windows)]
pub mod winapi;

pub use engine::Engine;
pub use error::{CoreError, CoreResult};
