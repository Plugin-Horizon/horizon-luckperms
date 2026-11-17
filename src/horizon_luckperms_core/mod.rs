//! LuckPerms-compatible permissions for Horizon servers.
//!
//! This crate holds everything that does not need the engine: the data model and LuckPerms'
//! resolution rules (`calc`), its MariaDB schema (`storage::sql`), the SQL messaging service
//! (`messaging`), the `/lp` commands (`command`) and the runtime that ties them to a server
//! (`runtime`). The Horizon glue (the mixin helpers next to `horizon-mixin.toml`) only hands it
//! players, commands and ticks.
//!
//! Other mods ask it for permissions and meta through [`api`].

pub mod actionlog;
pub mod api;
pub mod bytebin;
pub mod calc;
pub mod command;
pub mod config;
pub mod context;
pub mod duration;
pub mod glue;
pub mod known;
pub mod lang;
pub mod messaging;
pub mod model;
pub mod node;
pub mod original;
pub mod runtime;
pub mod storage;
pub mod text;
pub mod uuid;
pub mod yaml;

pub use calc::Tristate;
pub use runtime::{Outgoing, Recipient};
