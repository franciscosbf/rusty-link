//! TODO:

#![allow(dead_code)] // TODO: remove this.

use futures_util::future::BoxFuture;

use crate::node::Node;
use crate::error::RustyError;
use crate::player::Player;
use crate::model::{TrackData, TrackEndReason, Milli, TrackException};

/// Event tag.
pub(crate) trait Event { }

/// Dispatched when a track starts playing.
#[allow(missing_docs)]
pub struct TrackStartEvent {
    /// Player where a track starts playing.
    pub player: Player,
    pub track: TrackData,
}

impl Event for TrackStartEvent { }

/// Dispatched when a track ends.
#[allow(missing_docs)]
pub struct TrackEndEvent {
    /// Player where a track ended.
    pub player: Player,
    pub track: TrackData,
    pub reason: TrackEndReason,
}

impl Event for TrackEndEvent { }

/// Dispatched when a track throws an exception.
#[allow(missing_docs)]
pub struct TrackExceptionEvent {
    /// Player where a track dispatched an exception.
    pub player: Player,
    pub track: TrackData,
    pub exception: TrackException,
}

impl Event for TrackExceptionEvent { }

/// Dispatched when a track gets stuck while playing.
#[allow(missing_docs)]
pub struct TrackStuckEvent {
    /// Player where the track got stuck.
    pub player: Player,
    pub track: TrackData,
    /// The threshold in milliseconds that was exceeded.
    pub threshold: Milli,
}

impl Event for TrackStuckEvent { }

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

/// Skeleton of event handlers.
///
/// Events are dispatched (i.e. calls to `EventHandlers::on_*`) as soon as their
/// are parsed and validated right after the web socket client has received them.
///
/// # Event Handlers Scheduling
///
/// Returned event handlers (i.e. boxed futures) aren't run in a deterministic
/// order. It's up to how the tokio runtime schedules each future.
///
/// # Performance Considerations
///
/// Most likely each `EventHandlers::on_*` has to do some setup before returning
/// the proper event handler. Try to minimize the impact of those operations,
/// otherwise you may end up flooding the internal dispatcher.
///
/// # Example
///
/// ```
/// # use std::sync::Arc;
/// # use rusty_link::event::*;
/// // FutureExt trait extension is necessary to get access to `boxed` method.
/// use futures_util::future::{BoxFuture, FutureExt};
///
/// # struct BotState { }
/// # impl BotState {
/// #     async fn register_started_track(&self, _ :TrackStartEvent) { }
/// # }
/// struct Handlers {
///     bot_state: Arc<BotState>,
/// }
///
/// impl EventHandlers for Handlers {
///     fn on_track_start(
///         &self,
///         event: TrackStartEvent
///     ) -> BoxFuture<'static, ()> {
///         let state = self.bot_state.clone();
///
///         async move {
///             state.register_started_track(event).await;
///         }.boxed()
///     }
///
///     // The same goes for the reamining handlers...
///     # fn on_track_end(
///     #     &self,
///     #     _: TrackEndEvent
///     # ) -> BoxFuture<'static, ()> {
///     #     async { }.boxed()
///     # }
///     #
///     # fn on_track_exception(
///     #     &self,
///     #     _: TrackExceptionEvent
///     # ) -> BoxFuture<'static, ()> {
///     #     async { }.boxed()
///     # }
///     #
///     # fn on_track_stuck(
///     #     &self,
///     #     _: TrackStuckEvent
///     # ) -> BoxFuture<'static, ()> {
///     #     async { }.boxed()
///     # }
///     #
///     # fn on_ws_closed(
///     #     &self,
///     #     _: WebSocketClosedEvent
///     # ) -> BoxFuture<'static, ()> {
///     #     async { }.boxed()
///     # }
///     #
///     # fn on_ws_error(
///     #     &self,
///     #     _: WebSocketErrorEvent
///     # ) -> BoxFuture<'static, ()> {
///     #     async {}.boxed()
///     # }
/// }
/// ```
pub trait EventHandlers {
    /// Receives the next [`TrackStartEvent`].
    fn on_track_start(
        &self,
        event: TrackStartEvent
    ) -> BoxFuture<'static, ()>;

    /// Receives the next [`TrackEndEvent`].
    fn on_track_end(
        &self,
        event: TrackEndEvent
    ) -> BoxFuture<'static, ()>;

    /// Receives the next [`TrackExceptionEvent`].
    fn on_track_exception(
        &self,
        event: TrackExceptionEvent
    ) -> BoxFuture<'static, ()>;

    /// Receives the next [`TrackStuckEvent`].
    fn on_track_stuck(
        &self,
        event: TrackStuckEvent
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
pub(crate) fn dispatch_event<H, E>(
    event: E,
    dispatcher: &H,
    handler: fn(&H, E) -> BoxFuture<'static, ()>
)
where
    H: EventHandlers,
    E: Event
{
    tokio::spawn(handler(dispatcher, event));
}
