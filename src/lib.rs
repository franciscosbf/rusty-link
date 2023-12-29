//! Standalone Lavalink client API.
//!
//! TODO:
//! [more detailed explanation]
//! [at least one code example that users can copy/paste to try it]
//! [even more advanced explanations if necessary]

#![deny(missing_docs)]
#![warn(missing_docs)]
#![doc(test(attr(deny(warnings))))]

pub(crate) mod penalty;
pub(crate) mod op;

pub mod event;
pub mod utils;
pub mod cache;
pub mod model;
pub mod filter;
pub mod node;
pub mod player;
pub mod queue;
pub mod error;

// To implement the event handler.
pub use async_trait::async_trait;
