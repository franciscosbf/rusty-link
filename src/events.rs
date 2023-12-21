//! TODO:

#![allow(dead_code)] // TODO: remove this.

use futures_util::future::BoxFuture;

use crate::node::Node;
use crate::error::RustyError;
use crate::player::Player;
use crate::models::{TrackData, TrackEndReason, Milli, TrackException};

/// Dispatched when a track starts playing
#[allow(missing_docs)]
pub struct TrackStartEvent<'a> {
    /// Player where a track starts playing.
    pub player: Player,
    pub track: TrackData<'a>,
}

/// Dispatched when a track ends
#[allow(missing_docs)]
pub struct TrackEndEvent<'a> {
    /// Player where a track ended.
    pub player: Player,
    pub track: TrackData<'a>,
    pub reason: TrackEndReason,
}

/// Dispatched when a track throws an exception
#[allow(missing_docs)]
pub struct TrackExceptionEvent<'a> {
    /// Player where a track dispatched an exception.
    pub player: Player,
    pub track: TrackData<'a>,
    pub exception: TrackException,
}

/// Dispatched when a track gets stuck while playing
#[allow(missing_docs)]
pub struct TrackStuckEvent<'a> {
    /// Player where the track got stuck.
    pub player: Player,
    pub track: TrackData<'a>,
    /// The threshold in milliseconds that was exceeded.
    pub threshold: Milli,
}

/// Dispatched when the websocket connection to Discord voice servers is closed.
pub struct WebSocketClosedEvent {
    /// Node where the web socket was closed.
    pub node: Node,
}

/// Special event used to report errors on receiving/parsing messages from the
/// web socket of a node.
#[allow(missing_docs)]
pub struct WebSocketErrorEvent {
    /// Node where the web socket got an error.
    pub node: Node,
    pub error: RustyError,
}

/// TODO: explain how to create handler.
pub trait EventHandlers<'a> {
    /// Receives the next [`TrackStartEvent`].
    fn on_track_start(&self, event: TrackStartEvent<'a>) -> BoxFuture<'a, ()>;
    /// Receives the next [`TrackEndEvent`].
    fn on_track_end(&self, event: TrackEndEvent<'a>) -> BoxFuture<'a, ()>;
    /// Receives the next [`TrackExceptionEvent`].
    fn on_track_exception(&self, event: TrackEndEvent<'a>) -> BoxFuture<'a, ()>;
    /// Receives the next [`TrackStuckEvent`].
    fn on_track_stuck(&self, event: TrackStuckEvent<'a>) -> BoxFuture<'a, ()>;
    /// Receives the next [`WebSocketClosedEvent`].
    fn on_ws_closed(&self, event: WebSocketClosedEvent) -> BoxFuture<'a, ()>;
    /// Receives the next [`WebSocketError`].
    fn on_ws_error(&self, event: WebSocketClosedEvent) -> BoxFuture<'a, ()>;
}

/// Processes the event with the given handler in a different task.
pub(crate) fn dispatch_event<'a: 'static, T>(
    event: T,
    handler: fn(T) -> BoxFuture<'a, ()>
) {
    tokio::spawn(handler(event));
}

// fn f() {
//     struct A<'a> {s: &'a str}
//     fn handler<'a>(a: A) -> BoxFuture<'a, ()> {
//         async move {
//             let _ = a;
//         }.boxed()
//     }
//     dispatch_event(A {s: ""}, handler);
// }
