//! TODO:

#![allow(dead_code)] // TODO: remove this.

use serde::Deserialize;

use crate::model::{
    DiscordAudioWsClosed, NodeStats, PlayerState, TrackData, TrackEndReason, TrackException,
};

#[derive(Deserialize, Debug)]
pub(crate) struct ReadyOp {
    pub(crate) resumed: bool,
    #[serde(rename = "sessionId")]
    pub(crate) session_id: String,
}

#[derive(Deserialize, Debug)]
pub(crate) struct UpdateOp<'a> {
    #[serde(rename = "guildId")]
    pub(crate) guild_id: &'a str,
    pub(crate) state: PlayerState,
}

#[derive(Deserialize, Debug)]
pub(crate) struct StatsOp {
    #[serde(flatten)]
    pub(crate) stats: NodeStats,
}

#[derive(Deserialize, Debug)]
pub(crate) struct TrackStartEventData {
    pub(crate) track: Box<TrackData>,
}

#[derive(Deserialize, Debug)]
pub(crate) struct TrackEndEventData {
    pub(crate) track: Box<TrackData>,
    pub(crate) reason: TrackEndReason,
}

#[derive(Deserialize, Debug)]
pub(crate) struct TrackExceptionEventData {
    pub(crate) track: Box<TrackData>,
    pub(crate) exception: TrackException,
}

#[derive(Deserialize, Debug)]
pub(crate) struct TrackStuckEventData {
    pub(crate) track: Box<TrackData>,
    #[serde(rename = "thresholdMs")]
    pub(crate) threshold: u64,
}

#[derive(Deserialize, Debug)]
#[serde(tag = "type")]
pub(crate) enum EventType {
    #[serde(rename = "TrackStartEvent")]
    TrackStart(TrackStartEventData),
    #[serde(rename = "TrackEndEvent")]
    TrackEnd(TrackEndEventData),
    #[serde(rename = "TrackExceptionEvent")]
    TrackException(TrackExceptionEventData),
    #[serde(rename = "TrackStuckEvent")]
    TrackStuck(TrackStuckEventData),
    #[serde(rename = "WebSocketClosedEvent")]
    WebSocketClosed(DiscordAudioWsClosed),
}

#[derive(Deserialize, Debug)]
pub(crate) struct EventOp<'a> {
    #[serde(rename = "guildId")]
    pub(crate) guild_id: &'a str,
    #[serde(flatten)]
    pub(crate) event: EventType,
}

#[derive(Deserialize, Debug)]
#[serde(tag = "op")]
#[serde(bound(deserialize = "'de: 'a"))]
pub(crate) enum OpType<'a> {
    #[serde(rename = "ready")]
    Ready(ReadyOp),
    #[serde(rename = "playerUpdate")]
    PlayerUpdate(UpdateOp<'a>),
    #[serde(rename = "stats")]
    Stats(StatsOp),
    #[serde(rename = "event")]
    Event(EventOp<'a>),
}
