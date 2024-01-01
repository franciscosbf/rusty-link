//! All error types that the API has.

// ############### NodeManagerError ###############

use thiserror::Error;
use tokio_tungstenite::tungstenite;

use crate::model::ApiError;

/// Parse error description.
pub type ParseDescription = String;

/// Client API errors.
#[derive(Error, Debug)]
pub enum RustyError {
    /// When selecting a node for a given Discord guild, this value is returned
    /// if there aren't registered nodes.
    #[error("there's any registered node")]
    WithoutNodes,
    /// When selecting a node for a given Discord guild, there's the possibility
    /// that all nodes aren't available due to unknown connection issues or the
    /// web socket isn't connected to them.
    #[error("there isn't any available node")]
    UnavailableNodes,
    /// When trying to insert a new node that's already registered.
    #[error("the node name that your are trying to register is in use")]
    DuplicatedNode,
    /// When couldn't find a node with a given identifier.
    #[error("name doesn't match any registered node")]
    MissingNode,
    /// The response content is corrupted.
    #[error("while trying to parse response: {0}")]
    ParseResponseError(#[source] reqwest::Error),
    /// The received web socket message is corrupted.
    #[error("while trying to parse response: {0}")]
    ParseSocketMessageError(#[source] serde_json::Error),
    /// The client received an error response from the node.
    #[error("error response returned by node: {0}")]
    InstanceError(ApiError),
    /// The returned error isn't related to some node operation.
    #[error("client API error: {0}")]
    RequestError(#[source] reqwest::Error),
    /// The web socket connection returned an error.
    #[error("web socket client error: {0}")]
    WebSocketError(#[source] tungstenite::Error),
    /// After establishing the web socket connection, the server didn't send a
    /// confirmation message.
    #[error("didn't got from node the ready operation message")]
    MissingReadyMessage,
    /// The server sent a close message.
    #[error("web socket client received a close message from node")]
    ImmediateWebSocketClose,
    /// When the web socket connection is already stablished.
    #[error("there's already a web socket running")]
    DuplicatedWebSocketError,
    /// There isn't an active connection with the node.
    #[error("client isn't connected to the node")]
    NotConnected,
}

pub use self::RustyError::*;
