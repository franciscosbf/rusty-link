//! Standalone Lavalink client API.
//!
//! TODO:
//! [more detailed explanation]
//! [at least one code example that users can copy/paste to try it]
//! [even more advanced explanations if necessary]

#![deny(missing_docs)]
#![warn(missing_docs)]
#![doc(test(attr(deny(warnings))))]

mod core;

pub mod cache;
pub mod models;
pub mod node;
pub mod player;
pub mod queue;
