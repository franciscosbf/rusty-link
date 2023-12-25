//! TODO:

#![allow(dead_code)] // TODO: remove this.

use std::sync::Arc;

use hashbrown::HashMap;
use tokio::net::TcpStream;
use tokio::task::JoinHandle;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use url::Url;
use reqwest::header::{HeaderMap, HeaderValue};
use tokio::sync::{oneshot, RwLock, Mutex};
use futures_util::StreamExt;
use tokio_tungstenite::tungstenite::protocol::Message;

use crate::error::RustyError;
use crate::model::{NodeStats, ApiError};
use crate::op::{ReadyOp, OpType};
use crate::player::Player;
use crate::utils::InnerArc;

/// Discord guild identifier.
pub type GuildId = String;
/// Discord bot user identifier.
pub type BotId = String;
/// Node unique identifier.
type NodeName = String;

const CLIENT_NAME: &str = "rusty-lava/0.1.0";

/// Connection info used at node registration.
#[allow(missing_docs)]
pub struct NodeInfo {
    /// Unique identifier used internally to query the node (it's what you
    /// want).
    pub name: NodeName,
    /// Tells if we want to resume session after the socket has been closed.
    pub preserve_session: bool,
    pub host: String,
    pub port: u16,
    pub secure: bool,
    pub password: String,
}

impl NodeInfo {
    fn parse_url(
        &self,
        base: &str,
        path: &str
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
}

struct InternalStats {
    data: Option<NodeStats>,
}

impl InternalStats {
    fn new() -> Self {
        Self { data: None }
    }

    /// Updates only if new uptime is greater than the current one.
    fn maybe_update(&mut self, new: &NodeStats) {
        if self.data.is_none() {
            self.data.replace(*new);
            return;
        }

        if self.data.as_ref().unwrap().uptime < new.uptime {
            self.data.replace(*new);
        }
    }
}

struct NodeConfig {
    bot_user_id: BotId,
    password: String,
    keep_session: bool,
    ws_url: Url,
    rest_url: Url,
}

struct NodeState {
    players: RwLock<HashMap<GuildId, Player>>,
    stats: RwLock<InternalStats>,
}

/// Gives node access to close web socket.
struct WebSocketAlert {
    last_session_id: String,
    tx: oneshot::Sender<()>,
    handler: JoinHandle<Result<(), RustyError>>,
}

impl WebSocketAlert {
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

/// TODO:
pub struct NodeRef {
    config: NodeConfig,
    state: Arc<NodeState>,
    client: reqwest::Client,
    ws_alerter: Mutex<Option<WebSocketAlert>>,
}

impl NodeRef {
    fn new(config: NodeConfig) -> Self {
        // Build headers for REST API client.
        let mut rest_headers = HeaderMap::new();
        rest_headers.insert(
            "Authorization",
            HeaderValue::from_str(&config.password).unwrap()
        );

        // Build REST client.
        let client = reqwest::Client::builder()
            .default_headers(rest_headers)
            .build()
            .expect(""); // TODO: write this msg.

        // Initialize internal state shared with the socket.
        let state = Arc::new(NodeState {
            players: RwLock::new(HashMap::new()),
            stats: RwLock::new(InternalStats::new())
        });

        Self {
            config, state, client, ws_alerter: Mutex::new(None),
        }
    }

    /// TODO:
    pub async fn get_stats(&self) -> Result<NodeStats, RustyError> {
        let url = self.config.rest_url.join("stats").unwrap();

        match self.client.get(url).send().await {
            Ok(response) => {
                if response.status() == reqwest::StatusCode::OK {
                    return match response.json::<NodeStats>().await {
                        Ok(stats) => {
                            // Tries to update the current uptime.
                            let mut current_stats = self.state.stats
                                .write()
                                .await;
                            current_stats.maybe_update(&stats);

                            Ok(stats)
                        },
                        Err(e) => Err(RustyError::ParseError(e.to_string())),
                    };
                }

                // Tries to parse the API error.
                match response.json::<ApiError>().await {
                    Ok(error) => Err(RustyError::InstanceError(error)),
                    Err(e) => Err(RustyError::ParseError(e.to_string())),
                }
            }
            Err(e) => Err(RustyError::RequestError(e)),
        }
    }

    /// TODO:
    pub async fn connect(&self) -> Result<bool, RustyError> {
        let mut ws_alerter = self.ws_alerter.lock().await;

        // Build partial web socket client request.
        let mut request_builder = http::Request::builder()
            .uri(self.config.ws_url.as_str())
            .header("User-Id", self.config.bot_user_id.as_str())
            .header("Authorization", self.config.password.as_str())
            .header("Client-Name", CLIENT_NAME);

        // Build request based on last session id (if any) and if we want to
        // resume the current local state.
        if ws_alerter.is_some() && self.config.keep_session {
            let session_id = ws_alerter
                .as_ref()
                .unwrap()
                .last_session_id
                .as_str();
            request_builder = request_builder
                .header("Session-Id", session_id);
        }
        let request = request_builder
            .body(())
            .unwrap();

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

        // Awaits for ready operation and tries to parse it.
        let raw = match ws_reader.process_next().await? {
            Some(raw) => raw,
            None => return Err(RustyError::MissingReadyMessage),
        };
        let ready_op = match serde_json::from_str::<ReadyOp>(raw.as_str()) {
            Ok(ready_op) => ready_op,
            Err(e) =>
                return Err(
                    RustyError::ParseError(
                        format!("invalid ready operation message: {e}")
                    )
                ),
        };

        // Prepare data structs to be moved to web socket receiver loop.
        let state = self.state.clone();
        let (tx, mut rx) = oneshot::channel();

        // Spawn web socket receiver loop.
        let handler = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = &mut rx => // Got notified to close the connection.
                        return match ws_reader.stream.close(None).await {
                            Ok(_) => break,
                            Err(e) => Err(RustyError::WebSocketError(e)),
                        },
                    result = ws_reader.process_next() => {
                        let raw = match result {
                            Ok(Some(contained_raw)) => contained_raw,
                            Ok(None) => // Terminates execution.
                                return Ok(()),
                            Err(e) => {
                                // TODO: dispatch event with error.
                                let _ = e;
                                break
                            },
                        };

                        // Tries to parse the operation.
                        let raw_str = raw.as_str();
                        let op = match serde_json::from_str::<OpType>(raw_str) {
                            Ok(op) => op,
                            Err(e) => {
                                // TODO: dispatch event with error.
                                let _ = e;
                                continue
                            },
                        };

                        // TODO: interpret operation.
                        let _ = op;
                        let _ = state;
                    }
                };
            }
            Ok(())
        });

        // Set web socket alerter so it can be explicitly closed later.
        let ws_notify = WebSocketAlert {
            last_session_id: ready_op.session_id, tx, handler
        };
        *ws_alerter = Some(ws_notify);

        Ok(ready_op.resumed)
    }

    /// If open, closes the web socket connection.
    ///
    /// Returns Ok if connection isn't open or the clone operation succeeded.
    pub async fn close(&self) -> Result<(), RustyError> {
        let mut ws_alerter = self.ws_alerter.lock().await;

        if ws_alerter.is_none() {
            return Ok(());
        }

        ws_alerter
            .take()
            .unwrap()
            .close()
            .await
    }
}

/// TODO:
#[derive(Clone)]
pub struct Node {
    inner: Arc<NodeRef>,
}

impl InnerArc for Node {
    type Ref = NodeRef;

    fn instance(&self) -> &Arc<Self::Ref> {
        &self.inner
    }
}

/// Keeps track of all registered nodes.
pub struct NodeManagerRef {
    bot_user_id: BotId,
    nodes: RwLock<HashMap<NodeName, Node>>,
    players: RwLock<HashMap<GuildId, Player>>,
}

impl NodeManagerRef {
    fn new(bot_user_id: BotId) -> Self {
        Self {
            bot_user_id,
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
        info: NodeInfo
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
pub struct NodeManager {
    inner: Arc<NodeManagerRef>,
}

impl NodeManager {
    /// Creates a NodeManager instance.
    pub fn new(bot_user_id: BotId) -> Self {
        Self { inner: Arc::new(NodeManagerRef::new(bot_user_id)) }
    }
}

impl InnerArc for NodeManager {
    type Ref = NodeManagerRef;

    fn instance(&self) -> &Arc<Self::Ref> {
        &self.inner
    }
}

