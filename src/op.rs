//! TODO:

#![allow(dead_code)] // TODO: remove this.

use serde::Deserialize;

#[derive(Deserialize, Debug)]
pub(crate) struct ReadyOp {
    pub(crate) resumed: bool,
    #[serde(rename = "sessionId")]
    pub(crate) session_id: String,
}

#[derive(Deserialize, Debug)]
#[serde(tag = "op")]
pub(crate) enum OpTypeOnly {
    #[serde(rename = "ready")]
    Ready(ReadyOp),
}

// TODO: remaining operations.

#[derive(Deserialize, Debug)]
#[serde(tag = "op")]
pub(crate) enum OpType {
    #[serde(rename = "ready")]
    Ready(ReadyOp),
    #[serde(rename = "playerUpdate")]
    PlayerUpdate,
    #[serde(rename = "stats")]
    Stats,
    #[serde(rename = "event")]
    Event,
}

