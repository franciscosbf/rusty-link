//! TODO:

#![allow(dead_code)] // TODO: remove this.

use std::sync::atomic::{AtomicBool, Ordering};
use std::{ops::Deref, sync::Arc};

use reqwest::header::{HeaderMap, HeaderValue};
use scc::HashMap;
use tokio::sync::RwLock;
use url::Url;

use crate::error::RustyError;
use crate::event::EventHandlers;
use crate::model::{CurrentSessionState, NewSessionState, NodeStats, Secs};
use crate::player::Player;
use crate::socket::{SessionGuard, Socket};
use crate::utils::{process_request, spawn_fut};

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
            "{}{}://{}:{}/v4{}",
            base,
            if self.secure { "s" } else { "" },
            self.host,
            self.port,
            path
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
    WebSocket,
    Endpoint,
}

/// Contains a [`reqwest::Client`] to issue HTTP operations in the node REST API.
pub(crate) struct Rest {
    url: Url,
    client: reqwest::Client,
}

impl Rest {
    pub(crate) fn url(&self) -> &Url {
        &self.url
    }

    /// Returns the HTTP client.
    pub(crate) fn client(&self) -> &reqwest::Client {
        &self.client
    }
}

/// TODO:
pub struct NodeRef {
    deleted: AtomicBool,
    players_to_remove: Arc<HashMap<GuildId, ()>>,
    players: Arc<HashMap<GuildId, Player>>,
    stats: RwLock<Option<NodeStats>>,
    rest: Rest,
    socket: Socket,
}

impl NodeRef {
    fn new(
        bot_user_id: String,
        password: String,
        ws_url: Url,
        rest_url: Url,
        handlers: Arc<dyn EventHandlers>,
        players: Arc<HashMap<GuildId, Player>>, // TODO: main players hash map from NodeManagerRef.
    ) -> Self {
        // Build headers for REST API client.
        let mut rest_headers = HeaderMap::new();
        rest_headers.insert("Authorization", HeaderValue::from_str(&password).unwrap());

        // Build REST client.
        let rest = Rest {
            url: rest_url,
            client: reqwest::Client::builder()
                .default_headers(rest_headers)
                .build()
                .expect(""), // TODO: write this msg.
        };

        let socket = Socket::new(ws_url, bot_user_id, password, handlers);

        Self {
            deleted: AtomicBool::new(false),
            players_to_remove: Arc::new(HashMap::new()),
            players,
            stats: RwLock::new(None),
            rest,
            socket,
        }
    }

    /// Invalidates the node (i.e. doesn't exist anymore).
    ///
    /// Its state is flagged as deleted since it might still referred in some player. Players that
    /// belong to this node are removed asynchronously as well.
    fn invalidate(&self) {
        self.deleted.store(true, Ordering::SeqCst);

        // Proceeds to remove all players from the global map in background.
        let players_to_remove = Arc::clone(&self.players_to_remove);
        let players = Arc::clone(&self.players);
        spawn_fut(async move {
            // Stops here if there are no players.
            let mut entry = players_to_remove.first_entry_async().await?;

            loop {
                // Removes the player.
                let guild = entry.key();
                players.remove_async(guild).await;

                // Moves to the next entry if any.
                entry = entry.next_async().await?;
            }

            #[allow(unreachable_code)]
            Option::<()>::None
        });
    }

    /// Returns true if the node no longer exists.
    pub(crate) fn invalid(&self) -> bool {
        self.deleted.load(Ordering::SeqCst)
    }

    /// Returns the player if its node is connected and the node isn't valid.
    pub(crate) async fn get_player(&self, guild_id: &str) -> Option<Player> {
        let player = self.players.get_async(guild_id).await?.get().clone();

        Some(player)
    }

    /// Removes the player if exists.
    pub(crate) async fn remove_player(&self, guild_id: &str) {
        self.players.remove_async(guild_id).await;
        self.players_to_remove.remove_async(guild_id).await;
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

    /// Reset current stats if any.
    pub(crate) async fn clear_stats(&self) {
        self.stats.write().await.take();
    }

    /// Return last stats update registered.
    async fn last_stats(&self) -> Option<NodeStats> {
        self.stats.read().await.as_ref().cloned()
    }

    /// HTTP client and url to perform REST operations.
    pub(crate) fn rest(&self) -> &Rest {
        &self.rest
    }

    /// Stores a reference to its wrapper.
    fn set_wrapper(&mut self, node: Node) {
        self.socket.node_ref(node);
    }

    /// TODO:
    async fn change_session_state<'a>(
        &self,
        session_guard: &SessionGuard<'a>,
        new_state: NewSessionState,
    ) -> Result<CurrentSessionState, RustyError> {
        let session_id = format!("sessions/{}", session_guard.id());
        let serialized_state = serde_json::to_vec(&new_state).unwrap();

        let url = self.rest.url().join(session_id.as_str()).unwrap();
        let request = self
            .rest
            .client()
            .patch(url)
            .header("Content-Type", "application/json")
            .body(serialized_state)
            .send();

        process_request(request).await
    }

    /// TODO:
    pub async fn preserve_session(&self) -> Result<CurrentSessionState, RustyError> {
        let session_guard = self.socket.soft_lock_session().await?;
        let new_state = NewSessionState::keep();

        let current_state = self.change_session_state(&session_guard, new_state).await?;

        session_guard.preserve_session();

        Ok(current_state)
    }

    /// TODO:
    pub async fn preserve_session_with_timeout(
        &self,
        timeout: Secs,
    ) -> Result<CurrentSessionState, RustyError> {
        let session_guard = self.socket.soft_lock_session().await?;
        let new_state = NewSessionState::keep_with_timeout(timeout);

        let current_state = self.change_session_state(&session_guard, new_state).await?;

        session_guard.preserve_session();

        Ok(current_state)
    }

    /// TODO:
    pub async fn discard_session(&self) -> Result<CurrentSessionState, RustyError> {
        let session_guard = self.socket.soft_lock_session().await?;
        let new_state = NewSessionState::reset();

        let current_state = self.change_session_state(&session_guard, new_state).await?;

        session_guard.discard_session();

        Ok(current_state)
    }

    /// TODO:
    pub async fn fetch_session_state(&self) -> Result<CurrentSessionState, RustyError> {
        let session_guard = self.socket.soft_lock_session().await?;
        let new_state = NewSessionState::read();

        let current_state = self.change_session_state(&session_guard, new_state).await?;

        Ok(current_state)
    }

    /// Returns true if the session is preserved or not on future connections.
    pub fn preserved_session(&self) -> bool {
        self.socket.preserved_session()
    }

    /// Returns the current session id if connected to the node.
    pub async fn current_session_id(&self) -> Option<String> {
        self.socket.session_id().await
    }

    /// TODO:
    pub async fn fetch_stats(&self) -> Result<NodeStats, RustyError> {
        let url = self.rest.url().join("stats").unwrap();
        let request = self.rest.client().get(url).send();

        let mut stats = process_request(request).await?;

        // Tries to update the current uptime.
        self.maybe_update_stats(&mut stats, StatsUpdater::Endpoint)
            .await;

        Ok(stats)
    }

    /// TODO:
    pub async fn get_stats(&self) -> Option<NodeStats> {
        self.last_stats().await
    }

    /// TODO:
    pub async fn connect(&self) -> Result<bool, RustyError> {
        self.socket.start().await
    }

    /// Returns true if there's an active web socket connection with the node.
    pub fn connected(&self) -> bool {
        self.socket.connected()
    }

    /// If open, closes the web socket connection.
    ///
    /// Returns Ok if succeeded or there isn't any connection.
    pub async fn close(&self) -> Result<(), RustyError> {
        self.socket.stop().await
    }
}

/// Holds the original node so you can use as if it was protected by an [`Arc`] instance.
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

impl Deref for Node {
    type Target = Arc<NodeRef>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

/// Keeps track of all registered nodes.
pub struct NodeManagerRef {
    bot_user_id: String,
    handlers: Arc<dyn EventHandlers>,
    nodes: HashMap<NodeName, Node>,
    players: Arc<HashMap<GuildId, Player>>,
}

impl NodeManagerRef {
    fn new(bot_user_id: BotId, handlers: Arc<dyn EventHandlers>) -> Self {
        Self {
            bot_user_id,
            handlers,
            nodes: HashMap::new(),
            players: Arc::new(HashMap::new()),
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
    pub async fn add_node(&self, _info: NodeConfig) -> Result<(), RustyError> {
        todo!()
    }

    /// TODO:
    pub async fn get_node(&self, _name: NodeName) -> Result<Node, RustyError> {
        todo!()
    }

    /// TODO:
    pub async fn remove_node(_name: NodeName) -> Result<(), RustyError> {
        todo!()
    }

    /// TODO:
    pub async fn get_player(&self, _guild: GuildId) -> Result<Player, RustyError> {
        // - tries to find
        // - inserts if not present
        // - checks if it was already present
        // - if yes then returns it
        // - check if node wasn't invalidated before returning the player
        // - if node is invalid, creates a new one.
        todo!()
    }

    /// TODO:
    pub async fn remove_player(&self, _guild: GuildId) -> Result<(), RustyError> {
        // - removes the player from the main map and the node's map where it belongs.
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
        H: EventHandlers + 'static,
    {
        let node_manager = NodeManagerRef::new(bot_user_id, Arc::new(handlers));
        Self {
            inner: Arc::new(node_manager),
        }
    }
}

impl Deref for NodeManager {
    type Target = Arc<NodeManagerRef>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

#[cfg(test)]
mod test {
    use std::{env, sync::Arc};

    use scc::HashMap;

    use crate::async_trait;
    use crate::event::*;
    use crate::node::NodeConfig;

    use super::{Node, NodeRef};

    struct HandlersMock;

    #[async_trait]
    impl EventHandlers for HandlersMock {
        async fn on_track_start(&self, _: TrackStartEvent) {} // TODO:
        async fn on_track_end(&self, _: TrackEndEvent) {} // TODO:
        async fn on_track_exception(&self, _: TrackExceptionEvent) {} // TODO:
        async fn on_track_stuck(&self, _: TrackStuckEvent) {} // TODO:
        async fn on_discord_ws_closed(&self, _: DiscordWsClosedEvent) {} // TODO:
        async fn on_ws_client_error(&self, _: WsClientErrorEvent) {} // TODO:
    }

    fn env_var(name: &str) -> String {
        match env::var(name) {
            Ok(val) => val,
            Err(_) => panic!("invalid environment variable `{name}`"),
        }
    }

    fn new_node() -> Node {
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
            Arc::new(HashMap::new()),
        );

        Node::new(Arc::new(inner))
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "requires a lavalink instance"]
    async fn web_socket_connect_and_close() {
        let node = new_node();

        let result = node.connect().await;
        assert!(
            result.is_ok(),
            "error from connect method: {}",
            result.unwrap_err()
        );
        assert!(!result.unwrap(), "received true");

        assert!(node.current_session_id().await.is_some());

        let result = node.close().await;
        assert!(
            result.is_ok(),
            "error from close method: {}",
            result.unwrap_err()
        );

        assert!(!node.connected());
        assert!(node.current_session_id().await.is_none());
    }

    #[tokio::test]
    #[ignore = "requires a lavalink instance"]
    async fn session_state_change_in_the_same_connection() {
        let node = new_node();

        node.connect()
            .await
            .expect("Failed to connect to the node.");

        assert!(!node.preserved_session());

        let result = node.preserve_session().await;
        assert!(
            result.is_ok(),
            "error from preserve_session: {}",
            result.unwrap_err()
        );
        let state = result.unwrap();
        assert!(
            state.resuming,
            "resuming is false after session has been preserved"
        );
        assert_eq!(state.timeout, 60);
        assert!(
            node.preserved_session(),
            "local session state isn't true after preserving it"
        );

        let result = node.preserve_session_with_timeout(120).await;
        assert!(
            result.is_ok(),
            "error from preserve_session_with_timeout: {}",
            result.unwrap_err()
        );
        let state = result.unwrap();
        assert!(
            state.resuming,
            "resuming is false after session has been preserved with timeout"
        );
        assert_eq!(state.timeout, 120);
        assert!(
            node.preserved_session(),
            "local session state isn't true after preserving it with timeout"
        );

        let result = node.discard_session().await;
        assert!(
            result.is_ok(),
            "error from session_state_preserved: {}",
            result.unwrap_err()
        );
        let state = result.unwrap();
        assert!(
            !state.resuming,
            "resuming is true after session has been discarded"
        );
        assert_eq!(state.timeout, 120);
        assert!(
            !node.preserved_session(),
            "local session state isn't false after discarding"
        );

        node.close().await.expect("Unexpected error on close.");
    }

    #[tokio::test]
    #[ignore = "requires a lavalink instance"]
    async fn session_state_change_with_reconnection() {
        let node = new_node();

        node.connect()
            .await
            .expect("Failed to connect to the node.");

        let previous_session_id = node.current_session_id().await;

        node.preserve_session()
            .await
            .expect("Unexpected error while trying to preserve session.");

        node.close().await.expect("Unexpected error on close.");

        let result = node.connect().await;
        assert!(
            result.is_ok(),
            "error from connect method: {}",
            result.unwrap_err()
        );
        assert!(
            result.unwrap(),
            "received false which means that the session wasn't restored"
        );

        let current_session_id = node.current_session_id().await;
        assert_eq!(previous_session_id, current_session_id);

        node.close().await.expect("Unexpected error on close.");
    }
}
