//! TODO:

#![allow(dead_code)] // TODO: remove this.

use std::sync::Arc;

use hashbrown::HashMap;
use tokio::task::JoinHandle;
use url::Url;
use reqwest::header::{HeaderMap, HeaderValue};
use tokio::sync::{oneshot, RwLock};
use futures_util::StreamExt;
use tokio_tungstenite::tungstenite::{self, protocol::Message};

use crate::error::RustyError;
use crate::models::{NodeStats, ApiError};
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

struct WebSocket {
    last_session_id: Option<String>,
    shutdown: Option<oneshot::Sender<()>>,
    handler: Option<JoinHandle<Result<(), tungstenite::Error>>>,
}

impl WebSocket {
    async fn process_message(
        result: Result<Message, tungstenite::Error>
    ) -> Result<Option<serde_json::Value>, RustyError> {
        // Fetch message.
        let message: Message = match result {
            Ok(msg) => msg,
            Err(e) => {
                return Err(RustyError::WebSocketError(e));
            }
        };
        // Parse contents.
        match message {
            Message::Text(data) => {
                match serde_json::from_str(data.as_str()) {
                    Ok(content) => {
                        if !matches!(content, serde_json::Value::Object(_)) {
                            return Err(
                                RustyError::ParseError(
                                    "expecting json object".to_string()
                                )
                            );
                        }

                        Ok(Some(content))
                    },
                    Err(e) => Err(RustyError::ParseError(e.to_string()))
                }
            }
            Message::Close(_) => Err(RustyError::ImmediateWebSocketClose),
            _ => Ok(None),
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

/// TODO:
pub struct NodeRef {
    config: NodeConfig,
    client: reqwest::Client,
    ws: WebSocket,
    state: Arc<NodeState>,
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

        // Finalize web socket state creation.
        let ws = WebSocket {
            last_session_id: None,

            // This fields are set by start_ws method.
            shutdown: None,
            handler: None
        };

        // Initialize internal state shared with the socket.
        let state = Arc::new(NodeState {
            players: RwLock::new(HashMap::new()),
            stats: RwLock::new(InternalStats::new())
        });

        Self { config, client, ws, state }
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

    // WARN: this function must be called only one time at node creation.
    async fn start_ws(&mut self) -> Result<(), RustyError> {
        // Build partial web socket client request.
        let partial_request = http::Request::builder()
            .uri(self.config.ws_url.as_str())
            .header("User-Id", self.config.bot_user_id.clone())
            .header("Authorization", self.config.password.clone())
            .header("Client-Name", CLIENT_NAME.to_string());

        // Build final request based on last session id and if we want to resume
        // the current local state.
        let last_session_id = &mut self.ws.last_session_id;
        let keep_session = self.config.keep_session;
        let resume_session = last_session_id.is_some() && keep_session;
        let request = if resume_session {
            let session_id = last_session_id
                .take()
                .unwrap();
            partial_request
                .header("Session-Id", session_id)
                .body(())
                .unwrap()
        } else {
            partial_request
                .body(())
                .unwrap()
        };

        let res = tokio_tungstenite::connect_async(request).await;
        let (mut stream, _ /* handle response ??? */) = match res {
            Ok(content) => content,
            Err(e) => {
                return Err(RustyError::WebSocketError(e));
            }
        };

        let (shutdown, mut rx) = oneshot::channel();

        // Wait until first item is received (client is expecting ready op).
        let item = stream.next().await;
        let result = match item {
            Some(res) => res,
            None => {
                return Err(RustyError::MissingReadyMessage);
            }
        };

        // Parse the raw value to be checked right after.
        let value = match WebSocket::process_message(result).await? {
            Some(val) => val,
            None => {
                return Err(RustyError::MissingReadyMessage);
            }
        };
        // TODO: parse event.

        let state = self.state.clone();

        // Launch web socket reader.
        let handler = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = &mut rx => { // Got requested to close the connection.
                        return stream.close(None).await;
                    }
                    item = stream.next() => { // Process next item if any.
                        if item.is_none() { // We done were.
                            return Ok(());
                        }

                        let res = item.unwrap();
                        match res {
                            Ok(message) => {
                                // TODO: process message.
                                let _ = message;
                                let _ = state.players;
                                let _ = state.stats;
                            },
                            Err(e) => { // Something went wrong.
                                let _ = e;
                                // Dispatch an event about this.
                                // TODO: notify error.
                            }
                        }
                    }
                };
            }
        });

        // Set close channel to finalize web socket reader.
        self.ws.shutdown = Some(shutdown);
        self.ws.handler = Some(handler);

        Ok(())
    }

    // WARN: this function must be called only one time at node removal.
    async fn shutdown_ws(&mut self) -> Result<(), tungstenite::Error> {
        // Order shutdown.
        let _ = self.ws.shutdown
            .take()
            .unwrap()
            .send(());

        // Wait until it closes.
        self.ws.handler
            .take()
            .unwrap()
            .await
            .unwrap_or(Ok(())) // Shadows JoinError... should I handle it???
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

