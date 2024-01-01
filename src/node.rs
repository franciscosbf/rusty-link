//! TODO:

#![allow(dead_code)] // TODO: remove this.

use std::sync::{Arc, atomic::{AtomicBool, Ordering}};

use hashbrown::HashMap;
use tokio::{net::TcpStream, task::JoinHandle, sync::{oneshot, RwLock, RwLockWriteGuard}};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use url::Url;
use reqwest::header::{HeaderMap, HeaderValue};
use futures_util::StreamExt;
use tokio_tungstenite::tungstenite::{self, protocol::Message};

use crate::{error::RustyError, model::{CurrentSessionState, Secs}};
use crate::event::{
    EventHandlers,
    WsClientErrorEvent,
    DiscordWsClosedEvent,
    TrackStartEvent,
    TrackEndEvent,
    TrackExceptionEvent,
    TrackStuckEvent
};
use crate::model::{NodeStats, NewSessionState};
use crate::op::{ReadyOp, OpType, EventType};
use crate::player::Player;
use crate::utils::{InnerArc, process_request};

/// Discord guild identifier.
pub type GuildId = String;
/// Discord bot user identifier.
pub type BotId = String;
/// Node unique identifier.
type NodeName = String;

const CLIENT_NAME: &str = "rusty-lava/0.1.0";

/// Connection config used at node registration.
#[allow(missing_docs)]
pub struct NodeConfig {
    /// User friendly unique identifier used internally to query the node.
    pub name: NodeName,
    pub host: String,
    pub port: u16,
    pub secure: bool,
    pub password: String,
}

impl NodeConfig {
    fn parse_url(&self, base: &str, path: &str) -> Result<Url, url::ParseError> {
        let raw = format!(
            "{}{}://{}:{}/v4{}", base,
            if self.secure { "s" } else { "" },
            self.host, self.port, path
        );
        Url::parse(raw.as_str())
    }

    fn rest(&self) -> Result<Url, url::ParseError> {
        self.parse_url("http", "/")
    }

    fn ws(&self) -> Result<Url, url::ParseError> {
        self.parse_url("ws", "/websocket")
    }
}

/// Describes the action that must be taken on each WebSocketReader::process_next call.
enum MsgAction {
    /// Text data of the new message to be processed.
    Process(String),
    /// All that's not Message::{Text, Close} is marked as ignored,
    Ignore,
    /// There's no more messages.
    Finalize,
}

struct WebSocketReader {
    stream: WebSocketStream<MaybeTlsStream<TcpStream>>,
}

impl WebSocketReader {
    /// Returns the next text message content if any.
    async fn process_next(&mut self) -> Result<MsgAction, RustyError> {
        // Waits for the next message.
        let item = match self.stream.next().await {
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

    async fn close(&mut self) -> Result<(), RustyError> {
        match self.stream.close(None).await {
            Ok(_) => Ok(()),
            Err(e) => Err(RustyError::WebSocketError(e)),
        }
    }
}

/// General data that can be accessed by all node operations.
struct NodeInternals {
    bot_user_id: BotId,
    password: String,
    ws_url: Url,
    rest_url: Url,
    handlers: Arc<dyn EventHandlers>,
}

/// Indicate where the node stats update is comming from.
enum StatsUpdater {
    WebSocket, Endpoint
}

struct NodeState {
    connected: AtomicBool,
    players: RwLock<HashMap<GuildId, Player>>,
    stats: RwLock<Option<NodeStats>>,
}

impl NodeState {
    fn connected(&self) -> bool {
        self.connected.load(Ordering::Acquire)
    }

    fn mark_connected(&self) {
        self.connected.store(true, Ordering::Release)
    }

    fn mark_disconnected(&self) {
        self.connected.store(false, Ordering::Release)
    }
}

impl NodeState {
    fn new() -> Self {
        Self {
            connected: AtomicBool::new(false),
            players: RwLock::new(HashMap::new()),
            stats: RwLock::new(None),
        }
    }

    /// Reset current stats if any.
    async fn clear_stats(&self) {
        self.stats.write().await.take();
    }

    /// Return last stats update registered.
    async fn last_stats(&self) -> Option<NodeStats> {
        self.stats.read().await.as_ref().cloned()
    }

    /// Update stats if the uptime of `new` is greater than the current one.
    ///
    /// If `kind` is [`StatsUpdater::Endpoint`], preserves the [FrameStats] if present.
    ///
    /// If the current state contains frame stats, then `new` will have a copy
    /// of it.
    ///
    /// [FrameStats]: crate::model::FrameStats
    async fn maybe_update_stats(&self, new: &mut NodeStats, kind: StatsUpdater) {
        let mut stats = self.stats.write().await;

        if stats.is_none() {
            stats.replace(*new);
            return;
        }

        let stats_ref = stats.as_ref().unwrap();
        if stats_ref.uptime < new.uptime {
            if matches!(kind, StatsUpdater::Endpoint) && stats_ref.frame_stats.is_some() {
                // Preserve frame stats.
                new.frame_stats = stats_ref.frame_stats;
                // WARN: can I improve this by changing how NodeStats is parsed?
            }
            stats.replace(*new);
        }
    }
}

struct NodeSession {
    preserve: bool,
    id: String,
}

impl NodeSession {
    fn new(id: String) -> Self {
        Self { preserve: false, id }
    }

    fn id(&self) -> &str {
        self.id.as_str()
    }

    fn keep(&mut self) {
        self.preserve = true;
    }

    fn reset(&mut self) {
        self.preserve = false;
    }
}

/// Gives node access to close the current web socket connection.
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

struct WsConn {
    // May contain the last or current active session, if any.
    session: Option<NodeSession>,
    /// Refers to the current active session, if any.
    alerter: Option<WsAlerter>,
}

impl WsConn {
    fn new() -> Self {
        Self { session: None, alerter: None }
    }

    /// Assumes that there's a session.
    fn session(&self) -> &NodeSession {
        self.session.as_ref().unwrap()
    }

    /// Assumes that there's a session.
    fn session_mut(&mut self) -> &mut NodeSession {
        self.session.as_mut().unwrap()
    }
}

/// TODO:
pub struct NodeRef {
    node: Option<Node>,
    config: NodeInternals,
    state: Arc<NodeState>,
    client: reqwest::Client,
    ws_conn: Arc<RwLock<WsConn>>,
}

/// Dispatches an event and awaits for its execution.
async fn process_event(
    event_type: EventType,
    handlers: Arc<dyn EventHandlers>,
    node: Node,
    player: Player,
) {
    match event_type {
        EventType::TrackStart(data) => {
            handlers.on_track_start(TrackStartEvent {
                player, track: data.track,
            }).await
        }
        EventType::TrackEnd(data) => {
            handlers.on_track_end(TrackEndEvent {
                player, track: data.track, reason: data.reason
            }).await
        }
        EventType::TrackException(data) => {
            handlers.on_track_exception(TrackExceptionEvent {
                player, track: data.track, exception: data.exception
            }).await
        }
        EventType::TrackStuck(data) => {
            handlers.on_track_stuck(TrackStuckEvent {
                player, track: data.track, threshold: data.threshold
            }).await
        }
        EventType::WebSocketClosed(data) => {
            handlers.on_discord_ws_closed(DiscordWsClosedEvent {
                node, player, description: data
            }).await
        }
    };
}

impl NodeRef {
    fn new(config: NodeInternals) -> Self {
        // Build headers for REST API client.
        let mut rest_headers = HeaderMap::new();
        rest_headers.insert("Authorization", HeaderValue::from_str(&config.password).unwrap());

        // Build REST client.
        let client = reqwest::Client::builder().default_headers(rest_headers).build()
            .expect(""); // TODO: write this msg.

        let state = Arc::new(NodeState::new());

        let ws_conn = Arc::new(RwLock::new(WsConn::new()));

        Self { node: None, config, state, client, ws_conn }
    }

    /// Stores a reference to its wrapper.
    fn set_wrapper(&mut self, node: Node) {
        self.node.replace(node);
    }

    /// TODO:
    async fn change_session_state(
        &self, ws_conn: &WsConn, state: NewSessionState
    ) -> Result<CurrentSessionState, RustyError> {
        if !self.state.connected() {
            return Err(RustyError::NotConnected);
        }

        let session_id = format!("sessions/{}", ws_conn.session().id());
        let serialized_state = serde_json::to_vec(&state).unwrap();

        let url = self.config.rest_url.join(session_id.as_str()).unwrap();
        let request = self.client.patch(url).header("Content-Type", "application/json")
            .body(serialized_state).send();

        process_request(request).await
    }

    /// TODO:
    pub async fn preserve_session(&self) -> Result<CurrentSessionState, RustyError> {
        let mut ws_conn = self.ws_conn.write().await;
        let state = NewSessionState::keep();

        let current_state = self.change_session_state(&ws_conn, state).await?;

        // Set local state flag.
        ws_conn.session_mut().keep();

        Ok(current_state)
    }

    /// TODO:
    pub async fn preserve_session_with_timeout(
        &self, timeout: Secs
    ) -> Result<CurrentSessionState, RustyError> {
        let mut ws_conn = self.ws_conn.write().await;
        let state = NewSessionState::keep_with_timeout(timeout);

        let current_state = self.change_session_state(&ws_conn, state).await?;

        // Set local state flag.
        ws_conn.session_mut().keep();

        Ok(current_state)
    }

    /// TODO:
    pub async fn discard_session(&self) -> Result<CurrentSessionState, RustyError> {
        let mut ws_conn = self.ws_conn.write().await;
        let state = NewSessionState::reset();

        let current_state = self.change_session_state(&ws_conn, state).await?;

        // Unset local state flag.
        ws_conn.session_mut().reset();

        Ok(current_state)
    }

    /// TODO:
    pub async fn fetch_session_state(&self) -> Result<CurrentSessionState, RustyError> {
        let ws_conn = self.ws_conn.read().await;
        let state = NewSessionState::read();

        let current_state = self.change_session_state(&ws_conn, state).await?;

        Ok(current_state)
    }

    /// TODO:
    pub async fn session_state_preserved(&self) -> bool {
        let ws_conn = self.ws_conn.read().await;

        self.state.connected() && ws_conn.session().preserve
    }

    /// TODO:
    pub async fn fetch_stats(&self) -> Result<NodeStats, RustyError> {
        let url = self.config.rest_url.join("stats").unwrap();
        let request = self.client.get(url).send();

        let mut stats = process_request(request).await?;

        // Tries to update the current uptime.
        self.state.maybe_update_stats(&mut stats, StatsUpdater::Endpoint).await;

        Ok(stats)
    }

    /// TODO:
    pub async fn get_stats(&self) -> Option<NodeStats> {
        *self.state.stats.read().await
    }

    /// TODO:
    pub async fn connect(&self) -> Result<bool, RustyError> {
        let mut ws_conn = self.ws_conn.write().await;

        if self.state.connected() {
            // Doesn't make sense trying to connect again.
            return Err(RustyError::DuplicatedWebSocketError);
        }

        // Build partial web socket client request.
        let mut request_builder = http::Request::builder()
            .uri(self.config.ws_url.as_str())
            // Web Socket base headers as per RFC 6455.
            .header("Host", self.config.ws_url.host_str().unwrap())
            .header("Connection", "Upgrade")
            .header("Upgrade", "websocket")
            .header("Sec-WebSocket-Key", tungstenite::handshake::client::generate_key())
            .header("Sec-WebSocket-Version", "13")
            // Lavalink required headers.
            .header("User-Id", self.config.bot_user_id.as_str())
            .header("Authorization", self.config.password.as_str())
            .header("Client-Name", CLIENT_NAME);

        // Build request based on last session id (if any) and if we want to resume the current
        // local state.
        match ws_conn.session {
            Some(ref session) if session.preserve =>
                request_builder = request_builder.header("Session-Id", session.id()),
            Some(_) => (), // Discard previous session that isn't meant to be preserved.
            None => (), // Ignored.
        }
        let request = request_builder.body(()).unwrap();

        // Try to stablish a connection.
        //
        // Response is ignored since we will verify if the session was restored after receiving
        // the first operation (aka ready op).
        let (stream, _) = match tokio_tungstenite::connect_async(request).await {
            Ok(content) => content,
            Err(e) => return Err(RustyError::WebSocketError(e)),
        };

        let mut ws_reader = WebSocketReader { stream };

        // Awaits for ready operation and parses it.
        let raw = match ws_reader.process_next().await? {
            MsgAction::Process(raw) => raw,
            action => {
                // Due to safety purposes, we check if the received message has an unexpected type
                // and if so the connection is closed before exiting.
                if let MsgAction::Ignore = action {
                    let _ = ws_reader.close().await;
                }

                return Err(RustyError::MissingReadyMessage);
            }
        };
        let resumed = match serde_json::from_str::<ReadyOp>(raw.as_str()) {
            Ok(ready_op) => {
                // Store current session id.
                ws_conn.session.replace(NodeSession::new(ready_op.session_id));
                ready_op.resumed
            }
            Err(e) =>
                return Err(RustyError::ParseSocketMessageError(e)),
        };

        // Prepare data structs to be moved to web socket receiver loop.
        let node = self.node.as_ref().unwrap().clone();
        let handlers = Arc::clone(&self.config.handlers);
        let state = Arc::clone(&self.state);
        let cws_conn = Arc::clone(&self.ws_conn);
        let (tx, mut rx) = oneshot::channel();

        self.state.mark_connected();

        // Spawn web socket receiver loop.
        let handler = tokio::spawn(async move {
            let mut normal_close_result = None;

            loop {
                tokio::select! {
                    _ = &mut rx => { // Got notified to close the connection.
                        normal_close_result = Some(ws_reader.close().await);
                        break
                    }
                    result = ws_reader.process_next() => {
                        let raw = match result {
                            Ok(MsgAction::Process(contained_raw)) => contained_raw,
                            Ok(MsgAction::Finalize) => break, // Terminates execution.
                            Ok(MsgAction::Ignore) => continue, // Unrecognized messages are ignored.
                            Err(e) => {
                                let chandlers = handlers.clone();
                                let event = WsClientErrorEvent {
                                    node: node.clone(), error: Box::new(e),
                                };
                                tokio::spawn(async move {
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
                                tokio::spawn(async move {
                                    chandlers.on_ws_client_error(event).await;
                                });
                                continue
                            },
                        };

                        // Process operation.
                        match op_type {
                            OpType::PlayerUpdate(op) => {
                                let players = state.players.read().await;
                                match players.get(op.guild_id) {
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
                                let players = state.players.read().await;
                                match players.get(op.guild_id) {
                                    Some(player) => {
                                        let chandlers = Arc::clone(&handlers);
                                        let cnode = node.clone();
                                        tokio::spawn(process_event(
                                            op.event, chandlers, cnode, player.clone()
                                        ));
                                    }
                                    None => continue,
                                }
                            }
                            _ => () // Ready op was already processed.
                        }
                    }
                };
            }

            state.mark_disconnected();

            // Reset web socket reader alerter on ubnormal exit.
            if normal_close_result.is_none() {
                cws_conn.write().await.alerter.take();
            }

            // Reset node stats. After the socket has been closed, we don't know
            // if the node will keep operating or not.
            state.clear_stats().await;

            normal_close_result.unwrap_or_else(|| Ok(()))
        });

        // Set web socket alerter so it can be explicitly closed later.
        ws_conn.alerter = Some(WsAlerter { tx, handler });

        Ok(resumed)
    }

    /// Returns true if there's an active connection with the node.
    pub fn connected(&self) -> bool {
        self.state.connected()
    }

    /// If open, closes the web socket connection.
    ///
    /// Returns Ok if on success or there isn't any connection.
    pub async fn close(&self) -> Result<(), RustyError> {
        let mut ws_conn = self.ws_conn.write().await;

        if self.state.connected() {
            // There's no connection with the node.
            return Ok(());
        }

        ws_conn.alerter.take().unwrap().close().await
    }
}

/// TODO:
#[derive(Clone)]
pub struct Node {
    inner: Arc<NodeRef>,
}

impl Node {
    fn new(config: NodeInternals) -> Self {
        let node = Self { inner: Arc::new(NodeRef::new(config)) };

        // Stores a cloned reference of node in the inner instance.
        unsafe {
            let inner = &mut *(Arc::as_ptr(&node.inner) as *mut NodeRef);
            inner.set_wrapper(node.clone());
        }

        node
    }
}

impl InnerArc for Node {
    type Ref = NodeRef;

    fn instance(&self) -> &Arc<Self::Ref> {
        &self.inner
    }
}

/// Keeps track of all registered nodes.
pub struct NodeManagerRef<H: EventHandlers> {
    bot_user_id: BotId,
    handlers: Arc<H>,
    nodes: RwLock<HashMap<NodeName, Node>>,
    players: RwLock<HashMap<GuildId, Player>>,
}

impl<H: EventHandlers> NodeManagerRef<H> {
    fn new(bot_user_id: BotId, handlers: H) -> Self {
        Self {
            bot_user_id, handlers: Arc::new(handlers),
            nodes: RwLock::new(HashMap::new()),
            players: RwLock::new(HashMap::new()),
        }
    }

    async fn select_node(&self) -> Result<Node, RustyError> {
        todo!()
    }

    /// TODO:
    ///
    /// # Panics
    ///
    /// TODO: explain panic at reqwest::Client creation...
    pub async fn add_node(
        &self,
        info: NodeConfig
    ) -> Result<(), RustyError> {
        todo!()
    }

    /// TODO:
    pub async fn get_node(
        &self,
        name: NodeName,
    ) -> Result<Node, RustyError> {
        todo!()
    }

    /// TODO:
    pub async fn remove_node(
        name: NodeName
    ) -> Result<(), RustyError> {
        todo!()
    }

    /// TODO:
    pub async fn get_player(
        &self, guild: GuildId
    ) -> Result<Player, RustyError> {
        // TODO: creates it if not present.
        todo!()
    }

    /// TODO:
    pub async fn remove_player(
        &self, guild: GuildId
    ) -> Result<(), RustyError> {
        todo!()
    }

    /// TODO:
    pub async fn shutdown() {
        todo!()
    }
}

/// Holds a reference to the node manager.
///
/// This way, you don't need wrap with [`Arc`].
#[derive(Clone)]
pub struct NodeManager<H: EventHandlers> {
    inner: Arc<NodeManagerRef<H>>,
}

impl<H: EventHandlers> NodeManager<H> {
    /// Creates a NodeManager instance.
    pub fn new(bot_user_id: BotId, handlers: H) -> Self {
        let node_manager = NodeManagerRef::new(bot_user_id, handlers);
        Self { inner: Arc::new(node_manager) }
    }
}

impl<H: EventHandlers> InnerArc for NodeManager<H> {
    type Ref = NodeManagerRef<H>;

    fn instance(&self) -> &Arc<Self::Ref> {
        &self.inner
    }
}

#[cfg(test)]
mod test {
    use std::{env, sync::Arc};

    use crate::async_trait;
    use crate::node::{NodeInternals, Node, NodeConfig};
    use crate::utils::InnerArc;
    use crate::event::*;

    struct HandlersMock;

    #[async_trait]
    impl EventHandlers for HandlersMock {
        async fn on_track_start(&self, _: TrackStartEvent) { }
        async fn on_track_end(&self, _: TrackEndEvent) { }
        async fn on_track_exception(&self, _: TrackExceptionEvent) { }
        async fn on_track_stuck(&self, _: TrackStuckEvent) { }
        async fn on_discord_ws_closed(&self, _: DiscordWsClosedEvent) { }
        async fn on_ws_client_error(&self, _: WsClientErrorEvent) { }
    }

    fn env_var(name: &str) -> String {
        match env::var(name) {
            Ok(val) => val,
            Err(_) => panic!("invalid environment variable `{name}`"),
        }
    }

    fn new_node_info() -> NodeInternals {
        let config = NodeConfig {
            name: "".to_string(),
            host: env_var("HOST"),
            port: env_var("PORT").parse::<u16>().expect("invalid valid port"),
            secure: false,
            password: "".to_string(),
        };

        NodeInternals {
            bot_user_id: env_var("BOT_USER_ID"),
            password: env_var("PASSWORD"),
            ws_url: config.ws().expect("invalid web socket url"),
            rest_url: config.rest().expect("invalid web socket url"),
            handlers: Arc::new(HandlersMock),
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "requires a lavalink instance"]
    async fn test_web_socket_connect_and_close() {
        let node_info = new_node_info();
        let node = Node::new(node_info);

        let result = node.instance().connect().await;
        assert!(result.is_ok(), "error from connect method: {}", result.unwrap_err());
        assert!(!result.unwrap(), "received true");

        let result = node.instance().close().await;
        assert!(result.is_ok(), "error from close method: {}", result.unwrap_err());
    }

    #[tokio::test]
    #[ignore = "requires a lavalink instance"]
    async fn test_session_state_change() {
        let node_info = new_node_info();
        let node = Node::new(node_info);

        let result = node.instance().connect().await;
        assert!(result.is_ok(), "error from connect method: {}", result.unwrap_err());
        assert!(!result.unwrap(), "received true");

        let result = node.instance().preserve_session().await;
        assert!(result.is_ok(), "error from preserve_session: {}", result.unwrap_err());
        let state = result.unwrap();
        assert!(state.resuming, "resuming is false after session has been preserved");
        assert_eq!(state.timeout, 60);
        assert!(node.instance().session_state_preserved().await, "local session state isn't true after preserving it");

        let result = node.instance().preserve_session_with_timeout(120).await;
        assert!(result.is_ok(), "error from preserve_session_with_timeout: {}", result.unwrap_err());
        let state = result.unwrap();
        assert!(state.resuming, "resuming is false after session has been preserved with timeout");
        assert_eq!(state.timeout, 120);
        assert!(node.instance().session_state_preserved().await, "local session state isn't true after preserving it with timeout");

        let result = node.instance().discard_session().await;
        assert!(result.is_ok(), "error from session_state_preserved: {}", result.unwrap_err());
        let state = result.unwrap();
        assert!(!state.resuming, "resuming is true after session has been discarded");
        assert_eq!(state.timeout, 120);
        assert!(!node.instance().session_state_preserved().await, "local session state isn't false after discarding");

        let result = node.instance().close().await;
        assert!(result.is_ok(), "error from close method: {}", result.unwrap_err());
    }
}
