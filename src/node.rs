//! TODO:

#![allow(dead_code)] // TODO: remove this.

use std::sync::Arc;

use hashbrown::HashMap;
use tokio::sync::RwLock;
use url::Url;
use reqwest::header::{HeaderMap, HeaderValue};

use crate::error::RustyError;
use crate::socket::{Socket, CurrentStateLock};
use crate::event::EventHandlers;
use crate::model::{CurrentSessionState, Secs, NodeStats, NewSessionState};
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

/// Indicate where the node stats update is comming from.
pub(crate) enum StatsUpdater {
    WebSocket, Endpoint
}

pub(crate) struct NodeState {
    pub(crate) players: RwLock<HashMap<GuildId, Player>>,
    pub(crate) stats: RwLock<Option<NodeStats>>,
}

impl NodeState {
    // TODO: methods ro fetch player and stats.
}

impl NodeState {
    fn new() -> Self {
        Self {
            players: RwLock::new(HashMap::new()),
            stats: RwLock::new(None),
        }
    }

    /// Reset current stats if any.
    pub(crate) async fn clear_stats(&self) {
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
    pub(crate) async fn maybe_update_stats(&self, new: &mut NodeStats, kind: StatsUpdater) {
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

/// TODO:
pub struct NodeRef {
    state: Arc<NodeState>,
    rest_url: Url,
    client: reqwest::Client,
    socket: Socket,
}

impl NodeRef {
    fn new(
        bot_user_id: String,
        password: String,
        ws_url: Url,
        rest_url: Url,
        handlers: Arc<dyn EventHandlers>,
    ) -> Self {
        // Build headers for REST API client.
        let mut rest_headers = HeaderMap::new();
        rest_headers.insert("Authorization", HeaderValue::from_str(&password).unwrap());

        // Build REST client.
        let client = reqwest::Client::builder().default_headers(rest_headers).build()
            .expect(""); // TODO: write this msg.

        let state = Arc::new(NodeState::new());

        let socket = Socket::new(
            ws_url, bot_user_id, password,
            handlers, Arc::clone(&state)
        );

        Self { state, rest_url, client, socket }
    }

    /// Stores a reference to its wrapper.
    fn set_wrapper(&mut self, node: Node) {
        self.socket.node_ref(node);
    }

    /// TODO:
    async fn change_session_state(
        &self, session_controller: &impl CurrentStateLock, state: NewSessionState
    ) -> Result<CurrentSessionState, RustyError> {
        let session_id = format!("sessions/{}", session_controller.id());
        let serialized_state = serde_json::to_vec(&state).unwrap();

        let url = self.rest_url.join(session_id.as_str()).unwrap();
        let request = self.client.patch(url).header("Content-Type", "application/json")
            .body(serialized_state).send();

        process_request(request).await
    }

    /// TODO:
    pub async fn preserve_session(&self) -> Result<CurrentSessionState, RustyError> {
        let mut session_controller = self.socket.state_controller().await?;
        let state = NewSessionState::keep();

        let current_state = self.change_session_state(&session_controller, state).await?;

        // Set local state flag.
        session_controller.preserve();

        Ok(current_state)
    }

    /// TODO:
    pub async fn preserve_session_with_timeout(
        &self, timeout: Secs
    ) -> Result<CurrentSessionState, RustyError> {
        let mut session_controller = self.socket.state_controller().await?;
        let state = NewSessionState::keep_with_timeout(timeout);

        let current_state = self.change_session_state(&session_controller, state).await?;

        session_controller.preserve();

        Ok(current_state)
    }

    /// TODO:
    pub async fn discard_session(&self) -> Result<CurrentSessionState, RustyError> {
        let mut session_controller = self.socket.state_controller().await?;
        let state = NewSessionState::reset();

        let current_state = self.change_session_state(&session_controller, state).await?;

        session_controller.discard();

        Ok(current_state)
    }

    /// TODO:
    pub async fn fetch_session_state(&self) -> Result<CurrentSessionState, RustyError> {
        let session_controller = self.socket.state_reader().await?;
        let state = NewSessionState::read();

        let current_state = self.change_session_state(&session_controller, state).await?;

        Ok(current_state)
    }

    /// Returns true if the session is preserved or not on future connections.
    pub async fn preserved_session(&self) -> bool {
        self.socket.preserved_session().await
    }

    /// TODO:
    pub async fn fetch_stats(&self) -> Result<NodeStats, RustyError> {
        let url = self.rest_url.join("stats").unwrap();
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
        self.socket.start().await
    }

    /// Returns true if there's an active web socket connection with the node.
    pub async fn connected(&self) -> bool {
        self.socket.connected().await
    }

    /// If open, closes the web socket connection.
    ///
    /// Returns Ok if succeeded or there isn't any connection.
    pub async fn close(&self) -> Result<(), RustyError> {
        self.socket.stop().await
    }
}

/// Node controller that holds the original reference so you can use as if it was protected by an
/// [`Arc`] instance.
#[derive(Clone)]
pub struct Node {
    inner: Arc<NodeRef>,
}

impl Node {
    fn new(inner: Arc<NodeRef>) -> Self {
        let node = Self { inner };

        // Stores a cloned reference of it in the inner instance.
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
pub struct NodeManagerRef {
    bot_user_id: String,
    handlers: Arc<dyn EventHandlers>,
    nodes: RwLock<HashMap<NodeName, Node>>,
    players: RwLock<HashMap<GuildId, Player>>,
}

impl NodeManagerRef {
    fn new(bot_user_id: BotId, handlers: Arc<dyn EventHandlers>) -> Self {
        Self {
            bot_user_id, handlers,
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
pub struct NodeManager {
    inner: Arc<NodeManagerRef>,
}

impl NodeManager {
    /// Creates a NodeManager instance.
    pub fn new<H>(bot_user_id: BotId, handlers: H) -> Self
    where
        H: EventHandlers + 'static
    {
        let node_manager = NodeManagerRef::new(bot_user_id, Arc::new(handlers));
        Self { inner: Arc::new(node_manager) }
    }
}

impl InnerArc for NodeManager {
    type Ref = NodeManagerRef;

    fn instance(&self) -> &Arc<Self::Ref> {
        &self.inner
    }
}

#[cfg(test)]
mod test {
    use std::{env, sync::Arc};

    use crate::{async_trait, utils::InnerArc};
    use crate::node::NodeConfig;
    use crate::event::*;

    use super::{NodeRef, Node};

    struct HandlersMock;

    #[async_trait]
    impl EventHandlers for HandlersMock {
        async fn on_track_start(&self, _: TrackStartEvent) { } // TODO:
        async fn on_track_end(&self, _: TrackEndEvent) { } // TODO:
        async fn on_track_exception(&self, _: TrackExceptionEvent) { } // TODO:
        async fn on_track_stuck(&self, _: TrackStuckEvent) { } // TODO:
        async fn on_discord_ws_closed(&self, _: DiscordWsClosedEvent) { } // TODO:
        async fn on_ws_client_error(&self, _: WsClientErrorEvent) { } // TODO:
    }

    fn env_var(name: &str) -> String {
        match env::var(name) {
            Ok(val) => val,
            Err(_) => panic!("invalid environment variable `{name}`"),
        }
    }

    fn new_node() -> Node{
        let config = NodeConfig {
            name: "".to_string(),
            host: env_var("HOST"),
            port: env_var("PORT").parse::<u16>().expect("invalid valid port"),
            secure: false,
            password: "".to_string(),
        };

        let inner = NodeRef::new(
            env_var("BOT_USER_ID"),
            env_var("PASSWORD"),
            config.ws().expect("invalid web socket url"),
            config.rest().expect("invalid web socket url"),
            Arc::new(HandlersMock),
        );

        Node::new(Arc::new(inner))
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "requires a lavalink instance"]
    async fn test_web_socket_connect_and_close() {
        let node = new_node();
        let instance = node.instance();

        let result = instance.connect().await;
        assert!(result.is_ok(), "error from connect method: {}", result.unwrap_err());
        assert!(!result.unwrap(), "received true");

        let result = instance.close().await;
        assert!(result.is_ok(), "error from close method: {}", result.unwrap_err());
    }

    #[tokio::test]
    #[ignore = "requires a lavalink instance"]
    async fn test_session_state_change() {
        let node = new_node();
        let instance = node.instance();

        let result = instance.connect().await;
        assert!(result.is_ok(), "error from connect method: {}", result.unwrap_err());
        assert!(!result.unwrap(), "received true");

        let result = instance.preserve_session().await;
        assert!(result.is_ok(), "error from preserve_session: {}", result.unwrap_err());
        let state = result.unwrap();
        assert!(state.resuming, "resuming is false after session has been preserved");
        assert_eq!(state.timeout, 60);
        assert!(instance.preserved_session().await, "local session state isn't true after preserving it");

        let result = instance.preserve_session_with_timeout(120).await;
        assert!(result.is_ok(), "error from preserve_session_with_timeout: {}", result.unwrap_err());
        let state = result.unwrap();
        assert!(state.resuming, "resuming is false after session has been preserved with timeout");
        assert_eq!(state.timeout, 120);
        assert!(instance.preserved_session().await, "local session state isn't true after preserving it with timeout");

        let result = instance.discard_session().await;
        assert!(result.is_ok(), "error from session_state_preserved: {}", result.unwrap_err());
        let state = result.unwrap();
        assert!(!state.resuming, "resuming is true after session has been discarded");
        assert_eq!(state.timeout, 120);
        assert!(!instance.preserved_session().await, "local session state isn't false after discarding");

        let result = instance.close().await;
        assert!(result.is_ok(), "error from close method: {}", result.unwrap_err());
    }
}
