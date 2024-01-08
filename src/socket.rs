//! TODO:

#![allow(dead_code)] // TODO: remove this.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use futures_util::StreamExt;
use tokio::net::TcpStream;
use tokio::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};
use tokio::{sync::oneshot, task::JoinHandle};
use tokio_tungstenite::tungstenite::{self, protocol::Message};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use url::Url;

use crate::error::RustyError;
use crate::event::{
    DiscordWsClosedEvent, EventHandlers, TrackEndEvent, TrackExceptionEvent, TrackStartEvent,
    TrackStuckEvent, WsClientErrorEvent,
};
use crate::node::{Node, StatsUpdater};
use crate::op::{EventType, OpType, ReadyOp};
use crate::player::Player;
use crate::utils::spawn_fut;

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
            spawn_fut(async move {
                handlers
                    .on_track_start(TrackStartEvent {
                        player,
                        track: data.track,
                    })
                    .await;
            });
        }
        EventType::TrackEnd(data) => {
            spawn_fut(async move {
                handlers
                    .on_track_end(TrackEndEvent {
                        player,
                        track: data.track,
                        reason: data.reason,
                    })
                    .await;
            });
        }
        EventType::TrackException(data) => {
            spawn_fut(async move {
                handlers
                    .on_track_exception(TrackExceptionEvent {
                        player,
                        track: data.track,
                        exception: data.exception,
                    })
                    .await;
            });
        }
        EventType::TrackStuck(data) => {
            spawn_fut(async move {
                handlers
                    .on_track_stuck(TrackStuckEvent {
                        player,
                        track: data.track,
                        threshold: data.threshold,
                    })
                    .await;
            });
        }
        EventType::WebSocketClosed(data) => {
            spawn_fut(async move {
                handlers
                    .on_discord_ws_closed(DiscordWsClosedEvent {
                        node,
                        player,
                        description: data,
                    })
                    .await;
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

/// Used to communicate with the web socket reader.
struct Alerter {
    tx: oneshot::Sender<()>,
    handler: JoinHandle<Result<(), RustyError>>,
}

impl Alerter {
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

/// Session state with exclusive access on write operations.
struct SessionInternals {
    id: Option<String>,
    alerter: Option<Alerter>,
}

impl SessionInternals {
    fn new() -> Self {
        Self {
            id: None,
            alerter: None,
        }
    }
}

/// Wraps a read guard over session internals for shared access so the session id can be safely
/// read.
struct SessionViewGuard<'a>(RwLockReadGuard<'a, SessionInternals>);

impl<'a> SessionViewGuard<'a> {
    fn session_id(&self) -> Option<&str> {
        self.0.id.as_deref()
    }
}

/// Wraps a write guard over session internals and a reference to session.
///
/// Has the same operations of [`SessionViewGuard`] but lets you mark the session as open with
/// exclusive access due to write privilege.
struct SessionChangeGuard<'a> {
    session_ref: &'a Session,
    internals_guard: RwLockWriteGuard<'a, SessionInternals>,
}

impl<'a> SessionChangeGuard<'a> {
    fn session_id(&self) -> Option<&str> {
        self.internals_guard.id.as_deref()
    }

    /// Marks session as open.
    ///
    /// This operation is protected by a write guard of [`SessionInternals`] guard. It's necessary
    /// to ensure that there isn't any change in the session id and alerter, while trying to open a
    /// connection with the node.
    fn mark_as_open(&mut self, id: String, alerter: Alerter) {
        self.internals_guard.alerter.replace(alerter);
        self.internals_guard.id = Some(id);
        self.session_ref.connected.store(true, Ordering::SeqCst);
    }
}

struct Session {
    connected: AtomicBool,
    preserved: AtomicBool,
    internals: RwLock<SessionInternals>,
}

impl Session {
    fn new() -> Self {
        Self {
            connected: AtomicBool::new(false),
            preserved: AtomicBool::new(false),
            internals: RwLock::new(SessionInternals::new()),
        }
    }

    fn connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }

    fn preserved(&self) -> bool {
        self.preserved.load(Ordering::SeqCst)
    }

    fn preserve(&self) {
        self.preserved.store(true, Ordering::SeqCst);
    }

    fn discard(&self) {
        self.preserved.store(false, Ordering::SeqCst);
    }

    /// Tells to close the session if open.
    async fn wait_for_closing(&self) -> Result<(), RustyError> {
        if !self.connected() {
            return Ok(());
        }

        self.connected.store(false, Ordering::SeqCst);

        self.internals
            .write()
            .await
            .alerter
            .take()
            .unwrap()
            .close()
            .await?;

        Ok(())
    }

    /// Soft lock to read session internals only if connected to the node.
    async fn session_view_lock(&self) -> Result<SessionViewGuard, RustyError> {
        if !self.connected() {
            return Err(RustyError::NotConnected);
        }

        Ok(SessionViewGuard(self.internals.read().await))
    }

    /// Gives exclusive access to session internals.
    ///
    /// Connection state and session preservation can be manipulated as well.
    ///
    /// It allows to change everything, no matter if there's an active connection or not.
    async fn session_change_lock(&self) -> SessionChangeGuard {
        SessionChangeGuard {
            session_ref: self,
            internals_guard: self.internals.write().await,
        }
    }

    /// Set connection state to closed and remove the connection alerter.
    async fn mark_as_closed(&self) {
        self.connected.store(false, Ordering::SeqCst);
        self.internals.write().await.alerter.take();
    }
}

pub(crate) struct Socket {
    url: Url,
    bot_id: String,
    password: String,
    handlers: Arc<dyn EventHandlers>,
    session: Arc<Session>,
    node: Option<Node>,
}

/// Prevents from changing the current session.
pub(crate) struct SessionGuard<'a> {
    id_guard: SessionViewGuard<'a>,
    session_ref: Arc<Session>,
}

impl<'a> SessionGuard<'a> {
    pub(crate) fn id(&self) -> &str {
        // Assumes that is present.
        self.id_guard.session_id().unwrap()
    }

    pub(crate) fn preserve_session(&self) {
        self.session_ref.preserve();
    }

    pub(crate) fn discard_session(&self) {
        self.session_ref.discard();
    }
}

impl Socket {
    pub(crate) fn new(
        url: Url,
        bot_id: String,
        password: String,
        handlers: Arc<dyn EventHandlers>,
    ) -> Self {
        Self {
            url,
            bot_id,
            password,
            handlers,
            node: None,
            session: Arc::new(Session::new()),
        }
    }

    /// Stores a reference to the node wrapper that holds this socket internally.
    pub(crate) fn node_ref(&mut self, node: Node) {
        self.node = Some(node);
    }

    /// Spawns the web socket receiver loop.
    fn listen(&self, mut stream: Stream) -> Alerter {
        // NOTE: assumes that node_ref was already called.
        let node = self.node.as_ref().unwrap().clone();
        let handlers = Arc::clone(&self.handlers);
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
                                spawn_fut(async move {
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
                                spawn_fut(async move {
                                    chandlers.on_ws_client_error(event).await;
                                });
                                continue
                            },
                        };

                        // Process operation.
                        match op_type {
                            OpType::PlayerUpdate(op) => {
                                match node.get_player(op.guild_id).await {
                                    Some(player) => {
                                        // TODO:
                                        let _ = op;
                                        let _ = player;
                                    }
                                    None => continue,
                                }
                            }
                            OpType::Stats(mut op) =>
                                node.maybe_update_stats(
                                    &mut op.stats, StatsUpdater::WebSocket
                                ).await,
                            OpType::Event(op) => {
                                match node.get_player(op.guild_id).await {
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
                session.mark_as_closed().await;
            }

            // Reset node stats. After the socket has been closed, we don't know
            // if the node will keep operating or not.
            node.clear_stats().await;

            normal_close_result.unwrap_or_else(|| Ok(()))
        });

        Alerter { tx, handler }
    }

    /// Tells if it is connected or not.
    pub(crate) fn connected(&self) -> bool {
        self.session.connected()
    }

    /// Creates a connection if not open.
    pub(crate) async fn start(&self) -> Result<bool, RustyError> {
        if self.session.connected() {
            // Doesn't make sense trying to connect again.
            return Err(RustyError::DuplicatedWebSocketError);
        }

        let mut session_guard = self.session.session_change_lock().await;

        // Build partial web socket client request.
        let mut request_builder = http::Request::builder()
            .uri(self.url.as_str())
            // Web Socket base headers as per RFC 6455.
            .header("Host", self.url.host_str().unwrap())
            .header("Connection", "Upgrade")
            .header("Upgrade", "websocket")
            .header(
                "Sec-WebSocket-Key",
                tungstenite::handshake::client::generate_key(),
            )
            .header("Sec-WebSocket-Version", "13")
            // Lavalink required headers.
            .header("User-Id", self.bot_id.as_str())
            .header("Authorization", self.password.as_str())
            .header("Client-Name", CLIENT_NAME);

        // Build request based on last session id (if any) and if we want to resume the current
        // local state.
        match session_guard.session_id() {
            Some(id) if self.session.preserved() => {
                request_builder = request_builder.header("Session-Id", id)
            }
            Some(_) => (), // Discard previous session that isn't meant to be preserved.
            None => (),    // Ignored.
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
        match serde_json::from_str::<ReadyOp>(raw.as_str()) {
            Ok(ready_op) => {
                let session_alerter = self.listen(stream);
                session_guard.mark_as_open(ready_op.session_id, session_alerter);

                Ok(ready_op.resumed)
            }
            Err(e) => Err(RustyError::ParseSocketMessageError(e)),
        }
    }

    /// Closes the connection if open.
    pub(crate) async fn stop(&self) -> Result<(), RustyError> {
        self.session.wait_for_closing().await
    }

    /// Tells if session if tries to keep session or not.
    pub(crate) fn preserved_session(&self) -> bool {
        self.session.preserved()
    }

    /// Returns a non-exclusive session guard, if connection is open.
    ///
    /// This way, it's possible to lock multiple times, unless `start` or `end` is executing at the
    /// same time.
    pub(crate) async fn soft_lock_session(&self) -> Result<SessionGuard, RustyError> {
        Ok(SessionGuard {
            id_guard: self.session.session_view_lock().await?,
            session_ref: Arc::clone(&self.session),
        })
    }
}
