//! TODO:

#![allow(dead_code)] // TODO: remove this.

use futures_util::future::BoxFuture;

use crate::node::Node;
use crate::error::RustyError;
use crate::player::Player;
use crate::model::{
    TrackData,
    TrackEndReason,
    Milli,
    TrackException,
    DiscordAudioWsClosed
};

/// Dispatched when a track starts playing.
#[allow(missing_docs)]
pub struct TrackStartEvent {
    /// Player where a track starts playing.
    pub player: Player,
    pub track: Box<TrackData>,
}

/// Dispatched when a track ends.
#[allow(missing_docs)]
pub struct TrackEndEvent {
    /// Player where a track ended.
    pub player: Player,
    pub track: Box<TrackData>,
    pub reason: TrackEndReason,
}

/// Dispatched when a track throws an exception.
#[allow(missing_docs)]
pub struct TrackExceptionEvent {
    /// Player where a track dispatched an exception.
    pub player: Player,
    pub track: Box<TrackData>,
    pub exception: TrackException,
}

/// Dispatched when a track gets stuck while playing.
#[allow(missing_docs)]
pub struct TrackStuckEvent {
    /// Player where the track got stuck.
    pub player: Player,
    pub track: Box<TrackData>,
    /// The threshold in milliseconds that was exceeded.
    pub threshold: Milli,
}

/// Dispatched when the websocket audio connection to Discord is closed.
pub struct DiscordWsClosedEvent<H: EventHandlers> {
    /// Node where the web socket was closed.
    pub node: Node<H>,
    /// Explanation on why the audio web socket was closed.
    pub description: DiscordAudioWsClosed,
}

/// Special event used to report errors on receiving/parsing messages from the
/// web socket of a node.
#[allow(missing_docs)]
pub struct WsClientErrorEvent<H: EventHandlers> {
    /// Node where the web socket got an error.
    pub node: Node<H>,
    pub error: Box<RustyError>,
}

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
///     # fn on_discord_ws_closed<H: EventHandlers>(
///     #     &self,
///     #     _: DiscordWsClosedEvent<H>
///     # ) -> BoxFuture<'static, ()> {
///     #     async { }.boxed()
///     # }
///     #
///     # fn on_ws_client_error<H: EventHandlers>(
///     #     &self,
///     #     _: WsClientErrorEvent<H>
///     # ) -> BoxFuture<'static, ()> {
///     #     async { }.boxed()
///     # }
/// }
/// ```
pub trait EventHandlers: Send + Sync + 'static {
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

    /// Receives the next [`DiscordWsClosedEvent`].
    fn on_discord_ws_closed<H: EventHandlers>(
        &self,
        event: DiscordWsClosedEvent<H>
    ) -> BoxFuture<'static, ()>;

    /// Receives the next [`WsClientErrorEvent`].
    fn on_ws_client_error<H: EventHandlers>(
        &self,
        event: WsClientErrorEvent<H>
    ) -> BoxFuture<'static, ()>;
}
