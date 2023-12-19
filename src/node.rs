//!

#![allow(dead_code)] // TODO: remove this.

use std::sync::Arc;

use hashbrown::HashMap;
use tokio::task::JoinHandle;
use url::Url;
use reqwest::header::{HeaderMap, HeaderValue};
use tokio::sync::{oneshot, RwLock};
use futures_util::StreamExt;
use tokio_tungstenite::tungstenite;

use crate::error::NodeError;
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
    pub host: String,
    pub port: u16,
    pub secure: bool,
    pub password: String,
}

impl NodeInfo {
    fn parse_url(&self, base: &str, path: &str) -> Result<Url, url::ParseError> {
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
    request: Option<http::Request<()>>,
    shutdown: Option<oneshot::Sender<()>>,
    handler: Option<JoinHandle<Result<(), tungstenite::Error>>>,
}

struct Stats {
    data: NodeStats,
}

impl Stats {
    fn new(stats: NodeStats) -> Self {
        Self { data: stats }
    }

    /// Updates only if new uptime is greater than the current one.
    fn maybe_update(&mut self, new: &NodeStats) {
        if self.data.uptime < new.uptime {
            self.data = *new;
        }
    }
}

/// TODO:
pub struct NodeRef {
    base_rest_url: Url,
    client: reqwest::Client,
    ws: WebSocket,
    players: Arc<RwLock<HashMap<GuildId, Player>>>,
    stats: Arc<RwLock<Stats>>,
}

impl NodeRef {
    fn new(
        bot_user_id: BotId,
        lavalink_password: String,
        base_rest_url: Url,
        base_ws_url: Url,
        initial_stats: NodeStats, // TODO: returned at node creation.
    ) -> Self {
        // Build web socket client request.
        let ws_request = http::Request::builder()
            .uri(base_ws_url.as_str())
            .header("User-Id", bot_user_id)
            .header("Authorization", lavalink_password.clone())
            .header("Client-Name", CLIENT_NAME.to_string())
            .body(())
            .unwrap();
        let ws_request = Some(ws_request);

        // Build headers for REST API client.
        let mut rest_headers = HeaderMap::new();
        rest_headers.insert(
            "Authorization",
            HeaderValue::from_str(&lavalink_password).unwrap()
        );

        // Build REST client.
        let client = reqwest::Client::builder()
            .default_headers(rest_headers)
            .build()
            .expect(""); // TODO: write this msg.

        let ws = WebSocket {
            request: ws_request,
            shutdown: None,
            handler: None
        };

        Self {
            base_rest_url, client, ws,
            players: Arc::new(RwLock::new(HashMap::new())),
            stats: Arc::new(RwLock::new(Stats::new(initial_stats)))
        }
    }

    /// TODO:
    pub async fn get_stats(&self) -> Result<NodeStats, NodeError> {
        let url = self.base_rest_url.join("stats").unwrap();

        match self.client.get(url).send().await {
            Ok(response) => {
                if response.status() == reqwest::StatusCode::OK {
                    match response.json::<NodeStats>().await {
                        Ok(stats) => {
                            // Tries to update the current uptime.
                            let mut current_stats = self.stats.write().await;
                            current_stats.maybe_update(&stats);

                            Ok(stats)
                        },
                        Err(_) => Err(NodeError::ParseError),
                    }
                } else {
                    match response.json::<ApiError>().await {
                        Ok(error) => Err(NodeError::InstanceError(error)),
                        Err(_) => Err(NodeError::ParseError),
                    }
                }
            }
            Err(e) => Err(NodeError::RequestError(e)),
        }
    }

    // WARN: this function must be called only one time at node creation.
    async fn start_ws(&mut self) -> Result<(), tungstenite::Error> {
        let (mut stream, _) = tokio_tungstenite::connect_async(
            self.ws.request.take().unwrap()
        ).await?;

        let (shutdown, mut rx) = oneshot::channel();

        let players = self.players.clone();
        let stats = self.stats.clone();

        let handler = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = &mut rx => {
                        return stream.close(None).await;
                    }
                    msg = stream.next() => {
                        // TODO: process message
                        let _ = msg;
                        let _ = players;
                        let _ = stats;
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

    async fn select_node(&self) -> Result<Node, NodeError> {
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
    ) -> Result<(), NodeError> {
        todo!()
    }

    /// TODO:
    pub async fn get_node(
        &self,
        name: NodeName,
    ) -> Result<Node, NodeError> {
        todo!()
    }

    /// TODO:
    pub async fn remove_node(
        name: NodeName
    ) -> Result<(), NodeError> {
        todo!()
    }

    /// TODO:
    pub async fn get_player(
        &self, guild: GuildId
    ) -> Result<Player, NodeError> {
        // TODO: creates it if not present.
        todo!()
    }

    /// TODO:
    pub async fn remove_player(
        &self, guild: GuildId
    ) -> Result<(), NodeError> {
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

