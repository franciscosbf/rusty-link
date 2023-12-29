//! TODO:

#![allow(dead_code)] // TODO: remove this.

use std::sync::Arc;

use hashbrown::HashMap;
use tokio::{
    net::TcpStream,
    task::JoinHandle,
    sync::{oneshot, RwLock}
};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use url::Url;
use reqwest::header::{HeaderMap, HeaderValue};
use futures_util::StreamExt;
use tokio_tungstenite::tungstenite::{
    protocol::Message,
    handshake::client::generate_key
};

use crate::error::RustyError;
use crate::event::{
    EventHandlers,
    WsClientErrorEvent,
    DiscordWsClosedEvent,
    TrackStartEvent,
    TrackEndEvent,
    TrackExceptionEvent,
    TrackStuckEvent
};
use crate::model::NodeStats;
use crate::op::{ReadyOp, OpType, EventType};
use crate::player::Player;
use crate::utils::{InnerArc, process_request};

/// Discord guild identifier.
pub type GuildId = String;
/// Discord bot user identifier.
pub type BotId = String;
/// Node unique identifier.
type NodeName = String;

const CLIENT_HOST: &str = "rusty-lava";
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
    fn parse_url(
        &self, base: &str, path: &str
    ) -> Result<Url, url::ParseError> {
        let raw = format!(
            "{}{}://{}:{}/v4{}", base,
            if self.secure { "s" } else { "" },
            self.host, self.port, path
        );
        Url::parse(raw.as_str())
    }

    fn rest(&self) -> Result<Url, url::ParseError> {
        self.parse_url("http", "")
    }

    fn ws(&self) -> Result<Url, url::ParseError> {
        self.parse_url("ws", "/websocket")
    }
}

struct WebSocketReader {
    stream: WebSocketStream<MaybeTlsStream<TcpStream>>,
}

impl WebSocketReader {
    /// Returns the next text message content if any.
    async fn process_next(&mut self) -> Result<Option<String>, RustyError> {
        let item = match self.stream.next().await {
            Some(item) => item,
            None => return Ok(None),
        };

        let message = match item {
            Ok(message) => message,
            Err(e) => return Err(RustyError::WebSocketError(e)),
        };

        match message {
            Message::Text(data) => Ok(Some(data)),
            Message::Close(_) => Err(RustyError::ImmediateWebSocketClose),
            _ => Ok(None), // Ignore other types of message.
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
struct NodeInfo<H: EventHandlers> {
    bot_user_id: BotId,
    password: String,
    ws_url: Url,
    rest_url: Url,
    handlers: Arc<H>,
}

/// Simple marker to indicate where are the new node stats update comming from.
enum StatsUpKind {
    WebSocket, Endpoint
}

use self::StatsUpKind::*;

struct NodeState {
    players: RwLock<HashMap<GuildId, Player>>,
    stats: RwLock<Option<NodeStats>>,
}

impl NodeState {
    fn new() -> Self {
        Self {
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
    /// If `kind` is [`Endpoint`], preserves the [FrameStats] if present.
    ///
    /// If the current state contains frame stats, then `new` will have a copy
    /// of it.
    ///
    /// [FrameStats]: crate::model::FrameStats
    async fn maybe_update_stats(&self, new: &mut NodeStats, kind: StatsUpKind) {
        let mut stats = self.stats.write().await;

        if stats.is_none() {
            stats.replace(*new);
            return;
        }

        let stats_ref = stats.as_ref().unwrap();
        if stats_ref.uptime < new.uptime {
            if matches!(kind, Endpoint) && stats_ref.frame_stats.is_some() {
                // Preserve frame stats.
                new.frame_stats = stats_ref.frame_stats;
                // WARN: can I improve this by changing how NodeStats is parsed?
            }
            stats.replace(*new);
        }
    }
}

struct NodeSession {
    keep: bool,
    last: Option<String>,
}

impl NodeSession {
    fn new() -> Self {
        Self { keep: false, last: None }
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
    session: NodeSession,
    /// Refers to the current active session, if any.
    alerter: Option<WsAlerter>,
}

impl WsConn {
    fn new() -> Self {
        Self { session: NodeSession::new(), alerter: None }
    }
}

/// TODO:
pub struct NodeRef<H: EventHandlers> {
    node: Option<Node<H>>,
    config: NodeInfo<H>,
    state: Arc<NodeState>,
    client: reqwest::Client,
    ws_conn: Arc<RwLock<WsConn>>,
}

/// Dispatches an event and awaits for its execution.
async fn process_event<H: EventHandlers>(
    event_type: EventType,
    handlers: Arc<H>,
    node: Node<H>,
    player: Player,
) {
    match event_type {
        EventType::TrackStart(event) => {
            handlers.on_track_start(TrackStartEvent {
                player, track: event.track,
            }).await
        }
        EventType::TrackEnd(event) => {
            handlers.on_track_end(TrackEndEvent {
                player, track: event.track, reason: event.reason
            }).await
        }
        EventType::TrackException(event) => {
            handlers.on_track_exception(TrackExceptionEvent {
                player, track: event.track, exception: event.exception
            }).await
        }
        EventType::TrackStuck(event) => {
            handlers.on_track_stuck(TrackStuckEvent {
                player, track: event.track, threshold: event.threshold
            }).await
        }
        EventType::WebSocketClosed(description) => {
            handlers.on_discord_ws_closed(DiscordWsClosedEvent {
                node, player, description
            }).await
        }
    };
}

impl<H: EventHandlers + Clone> NodeRef<H> {
    fn new(config: NodeInfo<H>) -> Self {
        // Build headers for REST API client.
        let mut rest_headers = HeaderMap::new();
        rest_headers.insert(
            "Authorization", HeaderValue::from_str(&config.password).unwrap()
        );

        // Build REST client.
        let client = reqwest::Client::builder()
            .default_headers(rest_headers)
            .build()
            .expect(""); // TODO: write this msg.

        let state = Arc::new(NodeState::new());

        let ws_conn = Arc::new(RwLock::new(WsConn::new()));

        Self { node: None, config, state, client, ws_conn }
    }

    /// Stores a reference to its wrapper.
    fn set_wrapper(&mut self, node: Node<H>) {
        self.node.replace(node);
    }

    /// TODO:
    pub async fn fetch_stats(&self) -> Result<NodeStats, RustyError> {
        let url = self.config.rest_url.join("stats").unwrap();
        let request = self.client.get(url).send();

        let mut stats = process_request(request).await?;

        // Tries to update the current uptime.
        self.state.maybe_update_stats(&mut stats, StatsUpKind::Endpoint).await;

        Ok(stats)
    }

    /// TODO:
    pub async fn get_stats(&self) -> Option<NodeStats> {
        *self.state.stats.read().await
    }

    /// TODO:
    pub async fn connect(&self) -> Result<bool, RustyError> {
        let mut ws_conn = self.ws_conn.write().await;

        if ws_conn.alerter.is_some() {
            // Doesn't make sense trying to connect again.
            return Err(RustyError::DuplicatedWebSocketError);
        }

        // Build partial web socket client request.
        let mut request_builder = http::Request::builder()
            .uri(self.config.ws_url.as_str())
            // Web Socket base headers according to RFC 6455.
            .header("Host", CLIENT_HOST)
            .header("Connection", "Upgrade")
            .header("Upgrade", "websocket")
            .header("Sec-WebSocket-Key", generate_key())
            .header("Sec-WebSocket-Version", "13")
            // Lavalink required headers.
            .header("User-Id", self.config.bot_user_id.as_str())
            .header("Authorization", self.config.password.as_str())
            .header("Client-Name", CLIENT_NAME);

        // Build request based on last session id (if any) and if we want to
        // resume the current local state.
        if ws_conn.session.keep && ws_conn.session.last.is_some() {
            let session_id = ws_conn.session.last.as_ref().unwrap();
            request_builder = request_builder.header("Session-Id", session_id);
        }
        let request = request_builder.body(()).unwrap();

        // Try to stablish a connection.
        let res = tokio_tungstenite::connect_async(request).await;
        // Response is ignored since we will verify if session was restored
        // after receiving the first operation (aka ready op).
        let (stream, _) = match res {
            Ok(content) => content,
            Err(e) => {
                return Err(RustyError::WebSocketError(e));
            }
        };

        let mut ws_reader = WebSocketReader { stream };

        // Awaits for ready operation and parses it.
        let raw = match ws_reader.process_next().await? {
            Some(raw) => raw,
            None => return Err(RustyError::MissingReadyMessage),
        };
        let resumed = match serde_json::from_str::<ReadyOp>(raw.as_str()) {
            Ok(ready_op) => {
                // Store current session id.
                ws_conn.session.last.replace(ready_op.session_id);
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

        // Spawn web socket receiver loop.
        let handler = tokio::spawn(async move {
            let mut close_result = None;

            loop {
                tokio::select! {
                    _ = &mut rx => { // Got notified to close the connection.
                        close_result = Some(ws_reader.close().await);
                        break
                    }
                    result = ws_reader.process_next() => {
                        let raw = match result {
                            Ok(Some(contained_raw)) => contained_raw,
                            Ok(None) => break, // Terminates execution.
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
                                    &mut op.stats, StatsUpKind::WebSocket
                                ).await,
                            OpType::Event(op) => {
                                let players = state.players.read().await;
                                match players.get(op.guild_id) {
                                    Some(player) => {
                                        let chandlers = Arc::clone(&handlers);
                                        let cnode = node.clone();
                                        tokio::spawn(
                                            process_event(
                                                op.event, chandlers,
                                                cnode, player.clone()
                                            )
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

            // Reset web socket reader alerter.
            if close_result.is_none() {
                cws_conn.write().await.alerter.take();
            }

            // Reset node stats. After the socket has been closed, we don't know
            // if the node will keep operating or not.
            state.clear_stats().await;

            close_result.unwrap_or_else(|| Ok(()))
        });

        // Set web socket alerter so it can be explicitly closed later.
        ws_conn.alerter = Some(WsAlerter { tx, handler });

        Ok(resumed)
    }

    /// If open, closes the web socket connection.
    ///
    /// Returns Ok if on success or there isn't any connection.
    pub async fn close(&self) -> Result<(), RustyError> {
        let mut ws_conn = self.ws_conn.write().await;

        if ws_conn.alerter.is_none() {
            // There's no connection with the node.
            return Ok(());
        }

        ws_conn.alerter.take().unwrap().close().await
    }
}

/// TODO:
#[derive(Clone)]
pub struct Node<H: EventHandlers> {
    inner: Arc<NodeRef<H>>,
}

impl<H: EventHandlers + Clone> Node<H> {
    fn new(config: NodeInfo<H>) -> Self {
        let node = Self { inner: Arc::new(NodeRef::new(config)) };

        // Stores a cloned reference of node in the inner instance.
        unsafe {
            let inner = &mut *(Arc::as_ptr(&node.inner) as *mut NodeRef<H>);
            inner.set_wrapper(node.clone());
        }

        node
    }
}

impl<H: EventHandlers> InnerArc for Node<H> {
    type Ref = NodeRef<H>;

    fn instance(&self) -> &Arc<Self::Ref> {
        &self.inner
    }
}

/// Keeps track of all registered nodes.
pub struct NodeManagerRef<H: EventHandlers> {
    bot_user_id: BotId,
    handlers: Arc<H>,
    nodes: RwLock<HashMap<NodeName, Node<H>>>,
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

    async fn select_node(&self) -> Result<Node<H>, RustyError> {
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
    ) -> Result<Node<H>, RustyError> {
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

    use futures_util::FutureExt;

    use crate::event::EventHandlers;
    use crate::node::{NodeInfo, Node, NodeConfig};
    use crate::utils::InnerArc;

    #[derive(Clone)]
    struct HandlersMock;

    impl EventHandlers for HandlersMock {
        fn on_track_start(
                &self, _: crate::event::TrackStartEvent
            ) -> futures_util::future::BoxFuture<'static, ()> {
            async { }.boxed()
        }

        fn on_track_end(
                &self, _: crate::event::TrackEndEvent
            ) -> futures_util::future::BoxFuture<'static, ()> {
            async { }.boxed()
        }

        fn on_track_exception(
                &self, _: crate::event::TrackExceptionEvent
            ) -> futures_util::future::BoxFuture<'static, ()> {
            async { }.boxed()
        }

        fn on_track_stuck(
                &self, _: crate::event::TrackStuckEvent
            ) -> futures_util::future::BoxFuture<'static, ()> {
            async { }.boxed()
        }

        fn on_discord_ws_closed<H: EventHandlers>(
                &self, _: crate::event::DiscordWsClosedEvent<H>
            ) -> futures_util::future::BoxFuture<'static, ()> {
            async { }.boxed()
        }

        fn on_ws_client_error<H: EventHandlers>(
                &self, _: crate::event::WsClientErrorEvent<H>
            ) -> futures_util::future::BoxFuture<'static, ()> {
            async { }.boxed()
        }
    }

    fn env_var(name: &str) -> String {
        match env::var(name) {
            Ok(val) => val,
            Err(_) => panic!("invalid environment variable `{name}`"),
        }
    }

    fn new_node_info() -> NodeInfo<HandlersMock> {
        let config = NodeConfig {
            name: "".to_string(),
            host: env_var("HOST"),
            port: env_var("PORT").parse::<u16>().expect("invalid valid port"),
            secure: false,
            password: "".to_string(),
        };

        NodeInfo {
            bot_user_id: env_var("BOT_USER_ID"),
            password: env_var("PASSWORD"),
            ws_url: config.ws().expect("invalid web socket url"),
            rest_url: config.rest().expect("invalid web socket url"),
            handlers: Arc::new(HandlersMock),
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_web_socket_connect_and_close() {
        let node_info = new_node_info();
        let node = Node::new(node_info);

        let result = node.instance().connect().await;
        assert!(result.is_ok(), "got error from connect method: {}", result.unwrap_err());
        assert!(!result.unwrap(), "received true");

        let result = node.instance().close().await;
        assert!(result.is_ok(), "got error from close method: {}", result.unwrap_err());
    }
}
