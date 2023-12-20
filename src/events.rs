//! TODO:

#![allow(dead_code)] // TODO: remove this.

use futures_util::future::BoxFuture;

use tokio::sync::mpsc::Receiver;

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

/// Channel receivers per event type.
///
/// Every method `on_*` uses [`tokio::sync::mpsc::Receiver`] internally.
#[allow(missing_docs)]
pub struct EventChannelReceivers<'a> {
    pub(crate) track_start_recv: Option<Receiver<TrackStartEvent<'a>>>,
    pub(crate) track_end_recv: Option<Receiver<TrackEndEvent<'a>>>,
    pub(crate) track_exception_recv: Option<Receiver<TrackExceptionEvent<'a>>>,
    pub(crate) track_stuck_recv: Option<Receiver<TrackStuckEvent<'a>>>,
    pub(crate) ws_closed_recv: Option<Receiver<WebSocketClosedEvent>>,
    pub(crate) ws_error_recv: Option<Receiver<WebSocketErrorEvent>>,
}

impl<'a: 'static> EventChannelReceivers<'a> {
    fn dispatch_handler<F, T>(handler: F, mut channel: Receiver<T>)
    where
        T: 'a + Send,
        F: 'a + Send + Clone + FnOnce(T) -> BoxFuture<'a, ()>,
    {
        tokio::spawn(async move {
            while let Some(event) = channel.recv().await {
                tokio::spawn(handler.clone()(event));
            }
        });
    }

    /// Returns a stream to receive the next [`TrackStartEvent`].
    pub fn on_track_start<F>(&'a mut self, handler: F) -> bool
    where
        F: 'a + Send + Clone + FnOnce(TrackStartEvent<'a>) -> BoxFuture<'a, ()>,
    {
        if self.track_start_recv.is_none() {
            return false;
        }

        let channel = self.track_start_recv.take().unwrap();

        EventChannelReceivers::dispatch_handler(handler, channel);

        true
    }

    fn f(&'a mut self) { // TODO: remove and explain this in a comment.
        use std::sync::Arc;
        let a = Arc::new(1);
        let b = a.clone();
        use futures_util::FutureExt;
        let h = |event: TrackStartEvent<'_>| async move { let _ = event.track.encoded; println!("{b}"); }.boxed();
        self.on_track_start(h);
    }

    // /// Receives the next [`TrackEndEvent`].
    // pub fn on_track_end(
    //     &mut self
    // ) -> impl Future<Output = Option<TrackEndEvent<'a>>> + '_ {
    //     self.track_end_receiver.recv()
    // }
    //
    // /// Receives the next [`TrackExceptionEvent`].
    // pub fn on_track_exception(
    //     &mut self
    // ) -> impl Future<Output = Option<TrackExceptionEvent<'a>>> + '_ {
    //     self.track_exception_receiver.recv()
    // }
    //
    // /// Receives the next [`TrackStuckEvent`].
    // pub fn on_track_stuck(
    //     &mut self
    // ) -> impl Future<Output = Option<TrackStuckEvent<'a>>> + '_ {
    //     self.track_stuck_receiver.recv()
    // }
    //
    // /// Receives the next [`WebSocketClosedEvent`].
    // pub fn on_web_socket_closed(
    //     &mut self
    // ) -> impl Future<Output = Option<WebSocketClosedEvent>> + '_ {
    //     self.ws_closed_receiver.recv()
    // }
    //
    // /// Receives the next [`WebSocketError`].
    // pub fn on_ws_error(
    //     &mut self
    // ) -> impl Future<Output = Option<WebSocketErrorEvent>> + '_ {
    //     self.ws_error_receiver.recv()
    // }
}
