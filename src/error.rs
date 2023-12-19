//! All error types that the API has.

// ############### NodeManagerError ###############

use tokio_tungstenite::tungstenite;

use std::{fmt::{Display, Debug}, error::Error};

use crate::models::ApiError;

/// Collection of errors that [NodeManager](crate::node::NodeManager) might
/// return in some of its methods.
#[derive(Debug)]
pub enum NodeError {
    /// When selecting a node for a given Discord guild, this value is returned
    /// if there aren't registered nodes.
    Empty,
    /// When selecting a node for a given Discord guild, there's the possibility
    /// that all nodes aren't available due to unknown connection issues.
    Unavailable,
    /// When trying to insert a new node that's already registered.
    Duplicated,
    /// When couldn't find a node with a given identifier.
    Missing,
    /// The response content is corrupted.
    ParseError,
    /// The client received an error response from the node.
    InstanceError(ApiError),
    /// The returned error isn't related to some node operation.
    RequestError(reqwest::Error),
    /// The web socket connection returned an error.
    WebSocketError(tungstenite::Error),
}

impl Display for NodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            NodeError::Empty =>
                write!(f, "there aren't registered nodes"),
            NodeError::Unavailable =>
                write!(f, "all nodes aren't available"),
            NodeError::Duplicated =>
                write!(f, "duplicated node"),
            NodeError::Missing =>
                write!(f, "missing node"),
            NodeError::ParseError =>
                write!(f, "corrupted message"),
            NodeError::InstanceError(ref error) =>
                <ApiError as Display>::fmt(error, f),
            NodeError::RequestError(ref error) =>
                <reqwest::Error as Display>::fmt(error, f),
            NodeError::WebSocketError(ref error) =>
                <tungstenite::Error as Display>::fmt(error, f)
        }
    }
}

impl Error for NodeError { }
