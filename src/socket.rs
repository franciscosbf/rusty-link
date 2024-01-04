//! TODO:

#![allow(dead_code)] // TODO: remove this.

use std::sync::Arc;

use tokio::net::TcpStream;
use tokio::sync::{RwLock, RwLockWriteGuard, RwLockReadGuard};
use tokio::{task::JoinHandle, sync::oneshot};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use url::Url;
use futures_util::StreamExt;
use tokio_tungstenite::tungstenite::{self, protocol::Message};

use crate::error::RustyError;
use crate::event::{
    EventHandlers,
    WsClientErrorEvent,
    TrackStartEvent,
    TrackEndEvent,
    TrackExceptionEvent,
    TrackStuckEvent,
    DiscordWsClosedEvent
};
use crate::node::{NodeState, Node, StatsUpdater};
use crate::op::{ReadyOp, OpType, EventType};
use crate::player::Player;
use crate::utils::spawn;

const CLIENT_NAME: &str = "rusty-lava/0.1.0";

/// Describes the action that must be taken on each WebSocketReader::process_next call.
enum MsgAction {
    /// Text data of the new message to be processed.
    Process(String),
    /// All that's not Message::{Text, Close} is marked as ignored,
    Ignore,
    /// There's no more messages.
    Finalize,
}

fn process_node_event(
    event_type: EventType,
    handlers: Arc<dyn EventHandlers>,
    node: Node,
    player: Player,
) {
    match event_type {
        EventType::TrackStart(data) => {
            spawn(async move {
                handlers.on_track_start(TrackStartEvent {
                    player, track: data.track,
                }).await;
            });
        }
        EventType::TrackEnd(data) => {
            spawn(async move {
                handlers.on_track_end(TrackEndEvent {
                    player, track: data.track, reason: data.reason
                }).await;
            });
        }
        EventType::TrackException(data) => {
            spawn(async move {
                handlers.on_track_exception(TrackExceptionEvent {
                    player, track: data.track, exception: data.exception
                }).await;
            });
        }
        EventType::TrackStuck(data) => {
            spawn(async move {
                handlers.on_track_stuck(TrackStuckEvent {
                    player, track: data.track, threshold: data.threshold
                }).await;
            });
        }
        EventType::WebSocketClosed(data) => {
            spawn(async move {
                handlers.on_discord_ws_closed(DiscordWsClosedEvent {
                    node, player, description: data
                }).await;
            });
        }
    };
}

/// Wraps a [`WebSocketStream`].
struct Stream(WebSocketStream<MaybeTlsStream<TcpStream>>);

impl Stream {
    /// Returns the next text message content if any.
    async fn process_next(&mut self) -> Result<MsgAction, RustyError> {
        // Waits for the next message.
        let item = match self.0.next().await {
            Some(item) => item,
            None => return Ok(MsgAction::Finalize),
        };

        let message = match item {
            Ok(message) => message,
            Err(e) => return Err(RustyError::WebSocketError(e)),
        };

        match message {
            Message::Text(data) => Ok(MsgAction::Process(data)),
            Message::Close(_) => Err(RustyError::ImmediateWebSocketClose),
            _ => Ok(MsgAction::Ignore),
        }
    }

    /// Closes the connection.
    async fn close(&mut self) -> Result<(), RustyError> {
        match self.0.close(None).await {
            Ok(_) => Ok(()),
            Err(e) => Err(RustyError::WebSocketError(e)),
        }
    }
}

struct WsAlerter {
    tx: oneshot::Sender<()>,
    handler: JoinHandle<Result<(), RustyError>>,
}

impl WsAlerter {
    // Notifies web socket reader to close and awaits for its result.
    async fn close(self) -> Result<(), RustyError> {
        // Orders to shutdown, despite of if the handler has already terminated
        // or not.
        let _ = self.tx.send(());

        // Catch the returned value.
        match self.handler.await {
            Ok(result) => result,
            Err(_) => Ok(()), // WARN: Shadows JoinError... should I handle it???
        }
    }
}

struct Session {
    connected: bool,
    preserved: bool,
    id: Option<String>,
    alerter: Option<WsAlerter>,
}

impl Session {
    fn new() -> Self {
        Self {
            connected: false, preserved: false,
            id: None, alerter: None
        }
    }

    fn connected(&self) -> bool {
        self.connected
    }

    fn preserved(&self) -> bool {
        self.preserved
    }

    fn preserve(&mut self) {
        self.preserved = true;
    }

    fn discard(&mut self) {
        self.preserved = false;
    }

    /// Marks session as opened.
    fn mark_as_opened(&mut self, alerter: WsAlerter) {
        self.connected = true;
        self.alerter.replace(alerter);
    }

    /// Discards alerter.
    fn mark_as_closed(&mut self) {
        self.connected = false;
        self.alerter.take();
    }

    // Signals to close the session if open.
    async fn wait_for_closing(&mut self) -> Result<(), RustyError> {
        if !self.connected {
            return Ok(());
        }

        self.connected = false;
        self.alerter.take().unwrap().close().await
    }
}

pub(crate) trait SessionIdReader {
    /// Returns the current session id.
    fn id(&self) -> &str;
}

/// Used to change the session state and read the current session id.
pub(crate) struct CurrentStateController<'a>(RwLockWriteGuard<'a, Session>);

impl<'a> SessionIdReader for CurrentStateController<'a> {
    fn id(&self) -> &str {
        // Assumes that is present.
        self.0.id.as_ref().unwrap()
    }
}

impl<'a> CurrentStateController<'a> {
    /// Returns the current session id.
    pub(crate) fn id(&self) -> &str {
        // Assumes that there's an open session.
        self.0.id.as_ref().unwrap().as_str()
    }

    /// Tries to preserve session state on future connections.
    pub(crate) fn preserve(&mut self) {
        self.0.preserve();
    }

    /// Discards session state on future connections.
    pub(crate) fn discard(&mut self) {
        self.0.discard();
    }
}

/// Only allows to read the session id.
pub(crate) struct CurrentStateReader<'a>(RwLockReadGuard<'a, Session>);

impl<'a> SessionIdReader for CurrentStateReader<'a> {
    fn id(&self) -> &str {
        // Assumes that is present.
        self.0.id.as_ref().unwrap()
    }
}

pub(crate) struct Socket {
    url: Url,
    bot_id: String,
    password: String,
    handlers: Arc<dyn EventHandlers>,
    state: Arc<NodeState>,
    session: Arc<RwLock<Session>>,
    node: Option<Node>,
}

impl Socket {
    pub(crate) fn new(
        url: Url,
        bot_id: String,
        password: String,
        handlers: Arc<dyn EventHandlers>,
        state: Arc<NodeState>,
    ) -> Self {
        Self {
            url, bot_id, password, handlers, state, node: None,
            session: Arc::new(RwLock::new(Session::new()))
        }
    }

    /// Stores a reference to the node wrapper that holds this socket internally.
    pub(crate) fn node_ref(&mut self, node: Node) {
        self.node = Some(node);
    }

    // Spawns the web socket receiver loop.
    fn listen(&self, mut stream: Stream) -> WsAlerter {
        let node = self.node.as_ref().unwrap().clone();
        let handlers = Arc::clone(&self.handlers);
        let state = Arc::clone(&self.state);
        let session = Arc::clone(&self.session);
        let (tx, mut rx) = oneshot::channel();

        let handler = tokio::spawn(async move {
            let mut normal_close_result = None;

            loop {
                tokio::select! {
                    _ = &mut rx => { // Got notified to close the connection.
                        normal_close_result = Some(stream.close().await);
                        break
                    }
                    result = stream.process_next() => {
                        let raw = match result {
                            Ok(MsgAction::Process(contained_raw)) => contained_raw,
                            Ok(MsgAction::Finalize) => break, // Terminates execution.
                            Ok(MsgAction::Ignore) => continue, // Unrecognized messages are ignored.
                            Err(e) => {
                                let chandlers = handlers.clone();
                                let event = WsClientErrorEvent {
                                    node: node.clone(), error: Box::new(e),
                                };
                                spawn(async move {
                                    chandlers.on_ws_client_error(event).await;
                                });
                                break // This kind of error cannot be tolerated.
                            },
                        };

                        // Tries to parse the operation.
                        let raw_str = raw.as_str();
                        let result = serde_json::from_str::<OpType>(raw_str);
                        let op_type = match result {
                            Ok(op_type) => op_type,
                            Err(e) => {
                                let chandlers = handlers.clone();
                                let event = WsClientErrorEvent {
                                    node: node.clone(), error: Box::new(
                                        RustyError::ParseSocketMessageError(e)
                                    ),
                                };
                                spawn(async move {
                                    chandlers.on_ws_client_error(event).await;
                                });
                                continue
                            },
                        };

                        // Process operation.
                        match op_type {
                            OpType::PlayerUpdate(op) => {
                                match state.get_player(op.guild_id).await {
                                    Some(player) => {
                                        // TODO:
                                        let _ = op;
                                        let _ = player;
                                    }
                                    None => continue,
                                }
                            }
                            OpType::Stats(mut op) =>
                                state.maybe_update_stats(
                                    &mut op.stats, StatsUpdater::WebSocket
                                ).await,
                            OpType::Event(op) => {
                                match state.get_player(op.guild_id).await {
                                    Some(player) => {
                                        let chandlers = Arc::clone(&handlers);
                                        let cnode = node.clone();
                                        process_node_event(
                                            op.event, chandlers, cnode, player.clone()
                                        );
                                    }
                                    None => continue,
                                }
                            }
                            _ => () // Ready op was already processed.
                        }
                    }
                };
            }

            // Reset web socket reader alerter on ubnormal exit.
            if normal_close_result.is_none() {
                session.write().await.mark_as_closed();
            }

            // Reset node stats. After the socket has been closed, we don't know
            // if the node will keep operating or not.
            state.clear_stats().await;

            normal_close_result.unwrap_or_else(|| Ok(()))
        });

        WsAlerter { tx, handler }
    }

    /// Tells if it is connected or not.
    pub(crate) async fn connected(&self) -> bool {
        self.session.read().await.connected()
    }

    // Creates a connection if not open.
    pub(crate) async fn start(&self) -> Result<bool, RustyError> {
        let mut session = self.session.write().await;

        if session.connected {
            // Doesn't make sense trying to connect again.
            return Err(RustyError::DuplicatedWebSocketError);
        }

        // Build partial web socket client request.
        let mut request_builder = http::Request::builder()
            .uri(self.url.as_str())
            // Web Socket base headers as per RFC 6455.
            .header("Host", self.url.host_str().unwrap())
            .header("Connection", "Upgrade")
            .header("Upgrade", "websocket")
            .header("Sec-WebSocket-Key", tungstenite::handshake::client::generate_key())
            .header("Sec-WebSocket-Version", "13")
            // Lavalink required headers.
            .header("User-Id", self.bot_id.as_str())
            .header("Authorization", self.password.as_str())
            .header("Client-Name", CLIENT_NAME);

        // Build request based on last session id (if any) and if we want to resume the current
        // local state.
        match session.id {
            Some(ref id) if session.preserved =>
                request_builder = request_builder.header("Session-Id", id),
            Some(_) => (), // Discard previous session that isn't meant to be preserved.
            None => (), // Ignored.
        }
        let request = request_builder.body(()).unwrap();

        // Try to stablish a connection.
        //
        // Response is ignored since we will verify if the session was restored after receiving
        // the first operation (aka ready op).
        let mut stream = match tokio_tungstenite::connect_async(request).await {
            Ok((stream, _)) => Stream(stream),
            Err(e) => return Err(RustyError::WebSocketError(e)),
        };

        // Awaits for ready operation and parses it.
        let raw = match stream.process_next().await? {
            MsgAction::Process(raw) => raw,
            action => {
                // Due to safety purposes, we check if the received message has an unexpected type
                // and if so the connection is closed before exiting.
                if let MsgAction::Ignore = action {
                    let _ = stream.close().await;
                }

                return Err(RustyError::MissingReadyMessage);
            }
        };
        let resumed = match serde_json::from_str::<ReadyOp>(raw.as_str()) {
            Ok(ready_op) => {
                // Store current session id.
                session.id.replace(ready_op.session_id);
                ready_op.resumed
            }
            Err(e) =>
                return Err(RustyError::ParseSocketMessageError(e)),
        };

        session.mark_as_opened(self.listen(stream));

        Ok(resumed)
    }

    /// Closes the connection if open.
    pub(crate) async fn stop(&self) -> Result<(), RustyError> {
        self.session.write().await.wait_for_closing().await
    }

    /// Tells if session if tries to keep session or not.
    pub(crate) async fn preserved_session(&self) -> bool {
        self.session.read().await.preserved()
    }

    /// Returns a session state controller if connected.
    pub(crate) async fn state_controller(&self) -> Result<CurrentStateController, RustyError> {
        let session = self.session.write().await;

        if !session.connected() {
            return Err(RustyError::NotConnected);
        }

        Ok(CurrentStateController(session))
    }

    /// Returns a session state reader if connected.
    pub(crate) async fn state_reader(&self) -> Result<CurrentStateReader, RustyError> {
        let session = self.session.read().await;

        if !session.connected() {
            return Err(RustyError::NotConnected);
        }

        Ok(CurrentStateReader(session))
    }
}
