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
pub(crate) struct TrackStartEventData {
    pub(crate) track: TrackData,
}

#[derive(Deserialize, Debug)]
pub(crate) struct TrackEndEventData {
    pub(crate) track: TrackData,
    pub(crate) reason: TrackEndReason,
}

#[derive(Deserialize, Debug)]
pub(crate) struct TrackExceptionEventData {
    pub(crate) track: TrackData,
    pub(crate) exception: TrackException,
}

#[derive(Deserialize, Debug)]
pub(crate) struct TrackStuckEventData {
    pub(crate) track: TrackData,
    #[serde(rename = "thresholdMs")]
    pub(crate) threshold: Milli,
}

#[derive(Deserialize, Debug)]
#[serde(tag = "type")]
pub(crate) enum EventType {
    #[serde(rename = "TrackStartEvent")]
    TrackStart(Box<TrackStartEventData>),
    #[serde(rename = "TrackEndEvent")]
    TrackEnd(Box<TrackEndEventData>),
    #[serde(rename = "TrackExceptionEvent")]
    TrackException(Box<TrackExceptionEventData>),
    #[serde(rename = "TrackStuckEvent")]
    TrackStuck(Box<TrackStuckEventData>),
    #[serde(rename = "WebSocketClosedEvent")]
    WebSocketClosed(DiscordAudioWsClosed),
}

#[derive(Deserialize, Debug)]
pub(crate) struct EventOp {
    #[serde(rename = "guildId")]
    pub(crate) guild_id: GuildId,
    #[serde(flatten)]
    pub(crate) event: EventType,
}

#[derive(Deserialize, Debug)]
#[serde(tag = "op")]
pub(crate) enum OpType {
    #[serde(rename = "ready")]
    Ready(ReadyOp),
    #[serde(rename = "playerUpdate")]
    PlayerUpdate(Box<UpdateOp>),
    #[serde(rename = "stats")]
    Stats(Box<StatsOp>),
    #[serde(rename = "event")]
    Event(EventOp),
}

