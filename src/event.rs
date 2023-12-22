//! TODO:

#![allow(dead_code)] // TODO: remove this.

use futures_util::future::BoxFuture;

use crate::node::Node;
use crate::error::RustyError;
use crate::player::Player;
use crate::model::{TrackData, TrackEndReason, Milli, TrackException};

/// Event tag.
pub(crate) trait Event { }

/// Dispatched when a track starts playing
#[allow(missing_docs)]
pub struct TrackStartEvent<'a> {
    /// Player where a track starts playing.
    pub player: Player,
    pub track: TrackData<'a>,
}

impl<'a> Event for TrackStartEvent<'a> { }

/// Dispatched when a track ends
#[allow(missing_docs)]
pub struct TrackEndEvent<'a> {
    /// Player where a track ended.
    pub player: Player,
    pub track: TrackData<'a>,
    pub reason: TrackEndReason,
}

impl<'a> Event for TrackEndEvent<'a> { }

/// Dispatched when a track throws an exception
#[allow(missing_docs)]
pub struct TrackExceptionEvent<'a> {
    /// Player where a track dispatched an exception.
    pub player: Player,
    pub track: TrackData<'a>,
    pub exception: TrackException,
}

impl<'a> Event for TrackExceptionEvent<'a> { }

/// Dispatched when a track gets stuck while playing
#[allow(missing_docs)]
pub struct TrackStuckEvent<'a> {
    /// Player where the track got stuck.
    pub player: Player,
    pub track: TrackData<'a>,
    /// The threshold in milliseconds that was exceeded.
    pub threshold: Milli,
}

impl<'a> Event for TrackStuckEvent<'a> { }

/// Dispatched when the websocket connection to Discord voice servers is closed.
pub struct WebSocketClosedEvent {
    /// Node where the web socket was closed.
    pub node: Node,
}

impl Event for WebSocketClosedEvent { }

/// Special event used to report errors on receiving/parsing messages from the
/// web socket of a node.
#[allow(missing_docs)]
pub struct WebSocketErrorEvent {
    /// Node where the web socket got an error.
    pub node: Node,
    pub error: RustyError,
}

impl Event for WebSocketErrorEvent { }

/// TODO: explain how to create handler.
pub trait EventHandlers<'a> {
    /// Receives the next [`TrackStartEvent`].
    fn on_track_start(
        &self,
        event: TrackStartEvent<'a>
    ) -> BoxFuture<'static, ()>;
    /// Receives the next [`TrackEndEvent`].
    fn on_track_end(
        &self,
        event: TrackEndEvent<'a>
    ) -> BoxFuture<'static, ()>;
    /// Receives the next [`TrackExceptionEvent`].
    fn on_track_exception(
        &self,
        event: TrackExceptionEvent<'a>
    ) -> BoxFuture<'static, ()>;
    /// Receives the next [`TrackStuckEvent`].
    fn on_track_stuck(
        &self,
        event: TrackStuckEvent<'a>
    ) -> BoxFuture<'static, ()>;
    /// Receives the next [`WebSocketClosedEvent`].
    fn on_ws_closed(
        &self,
        event: WebSocketClosedEvent
    ) -> BoxFuture<'static, ()>;
    /// Receives the next [`WebSocketErrorEvent`].
    fn on_ws_error(
        &self,
        event: WebSocketErrorEvent
    ) -> BoxFuture<'static, ()>;
}

/// Processes the event with the given handler in a different task.
pub(crate) fn dispatch_event<'a, H, E>(
    event: E,
    dispatcher: &H,
    handler: fn(&H, E) -> BoxFuture<'static, ()>
)
where
    H: EventHandlers<'a>,
    E: Event
{
    tokio::spawn(handler(dispatcher, event));
}
