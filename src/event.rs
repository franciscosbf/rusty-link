//! TODO:

#![allow(dead_code)] // TODO: remove this.

use async_trait::async_trait;

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
pub struct DiscordWsClosedEvent {
    /// Node where the web socket was closed.
    pub node: Node,
    /// The player associated to the closed connection.
    pub player: Player,
    /// Explanation on why the audio web socket was closed.
    pub description: DiscordAudioWsClosed,
}

/// Special event used to report errors on receiving/parsing messages from the
/// web socket of a node.
#[allow(missing_docs)]
pub struct WsClientErrorEvent {
    /// Node where the web socket got an error.
    pub node: Node,
    pub error: Box<RustyError>,
}

/// Skeleton of event handlers.
///
/// Events are dispatched (i.e. calls to `EventHandlers::on_*`) as soon as their are parsed and
/// validated right after the web socket client has received them.
///
/// If the implementation of `EventHandlers` is composed of some other types, then they must
/// implement `Send` and `Sync` as well.
///
/// # Event Handlers Scheduling
///
/// Returned event handlers (i.e. boxed futures) aren't run in a deterministic order. It's up to
/// how the tokio runtime schedules each future.
///
/// # Performance Considerations
///
/// Most likely each `EventHandlers::on_*` has to do some setup before returning the proper event
/// handler. Try to minimize the impact of those operations, since the intention of each handler is
/// to run concurrently as soon as possible.
///
/// # Example
///
/// ```
/// # use std::sync::Arc;
/// # use rusty_link::event::*;
/// use rusty_link::async_trait;
///
/// # struct BotState;
/// # impl BotState {
/// #     async fn register_started_track(&self, _ :TrackStartEvent) { }
/// # }
/// struct Handlers {
///     bot_state: Arc<BotState>,
/// }
///
/// #[async_trait]
/// impl EventHandlers for Handlers {
///     async fn on_track_start(&self, event: TrackStartEvent) {
///         self.bot_state.register_started_track(event).await;
///     }
///
///     // The same goes for the reamining handlers...
///     # async fn on_track_end(&self, _: TrackEndEvent) { }
///     # async fn on_track_exception(&self, _: TrackExceptionEvent) { }
///     # async fn on_track_stuck(&self, _: TrackStuckEvent) { }
///     # async fn on_discord_ws_closed(&self, _: DiscordWsClosedEvent) { }
///     # async fn on_ws_client_error(&self, _: WsClientErrorEvent) { }
/// }
/// ```
#[async_trait]
pub trait EventHandlers: Send + Sync {
    /// Receives the next [`TrackStartEvent`].
    async fn on_track_start(&self, event: TrackStartEvent);

    /// Receives the next [`TrackEndEvent`].
    async fn on_track_end(&self, event: TrackEndEvent);

    /// Receives the next [`TrackExceptionEvent`].
    async fn on_track_exception(&self, event: TrackExceptionEvent);

    /// Receives the next [`TrackStuckEvent`].
    async fn on_track_stuck(&self, event: TrackStuckEvent);

    /// Receives the next [`DiscordWsClosedEvent`].
    async fn on_discord_ws_closed(&self, event: DiscordWsClosedEvent);

    /// Receives the next [`WsClientErrorEvent`].
    async fn on_ws_client_error(&self, event: WsClientErrorEvent);
}
