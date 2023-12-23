//! TODO:

#![allow(dead_code)] // TODO: remove this.

use serde::Deserialize;

use crate::node::GuildId;
use crate::model::{
    PlayerState,
    NodeStats,
    TrackData,
    TrackEndReason,
    TrackException,
    Milli,
    DiscordAudioWsClosed,
};

#[derive(Deserialize, Debug)]
pub(crate) struct ReadyOp {
    pub(crate) resumed: bool,
    #[serde(rename = "sessionId")]
    pub(crate) session_id: String,
}

#[derive(Deserialize, Debug)]
pub(crate) struct UpdateOp {
    #[serde(rename = "guildId")]
    pub(crate) guild_id: GuildId,
    pub(crate) state: PlayerState,
}

#[derive(Deserialize, Debug)]
pub(crate) struct StatsOp {
    #[serde(flatten)]
    pub(crate) stats: NodeStats,
}

#[derive(Deserialize, Debug)]
#[serde(tag = "type")]
pub(crate) enum EventType {
    #[serde(rename = "TrackStartEvent")]
    TrackStart {
        track: TrackData,
    },
    #[serde(rename = "TrackEndEvent")]
    TrackEnd {
        track: TrackData,
        reason: TrackEndReason,
    },
    #[serde(rename = "TrackExceptionEvent")]
    TrackException {
        track: TrackData,
        exception: TrackException,
    },
    #[serde(rename = "TrackStuckEvent")]
    TrackStuck {
        track: TrackData,
        #[serde(rename = "thresholdMs")]
        threshold: Milli,
    },
    #[serde(rename = "WebSocketClosedEvent")]
    WebSocketClosed(DiscordAudioWsClosed),
}

#[derive(Deserialize, Debug)]
pub(crate) struct EventOp {
    #[serde(rename = "guildId")]
    pub(crate) guild_id: GuildId,
    #[serde(flatten)]
    event: EventType,
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

