//! All error types that the API has.

// ############### NodeManagerError ###############

use tokio_tungstenite::tungstenite;

use std::{fmt::{Display, Debug}, error::Error};

use crate::model::ApiError;

/// Parse error description.
pub type ParseDescription = String;

/// Collection of errors that [NodeManager](crate::node::NodeManager) might
/// return in some of its methods.
#[derive(Debug)]
pub enum RustyError {
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
    ParseError(ParseDescription),
    /// The client received an error response from the node.
    InstanceError(ApiError),
    /// The returned error isn't related to some node operation.
    RequestError(reqwest::Error),
    /// The web socket connection returned an error.
    WebSocketError(tungstenite::Error),
    /// If after establishing the web socket connection, the server didn't send
    /// a confirmation message.
    MissingReadyMessage,
    /// The web socket returned an unexpected close message.
    ImmediateWebSocketClose,
    /// When the received message from web socket isn't of type text.
    UnexpectedMessage,
}

impl Display for RustyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            RustyError::Empty =>
                write!(f, "there aren't registered nodes"),
            RustyError::Unavailable =>
                write!(f, "all nodes aren't available"),
            RustyError::Duplicated =>
                write!(f, "duplicated node"),
            RustyError::Missing =>
                write!(f, "missing node"),
            RustyError::ParseError(ref desc) =>
                write!(f, "corrupted message: {}", desc),
            RustyError::InstanceError(ref error) =>
                <ApiError as Display>::fmt(error, f),
            RustyError::RequestError(ref error) =>
                <reqwest::Error as Display>::fmt(error, f),
            RustyError::WebSocketError(ref error) =>
                <tungstenite::Error as Display>::fmt(error, f),
            RustyError::MissingReadyMessage =>
                write!(f, "ready message not received by web socket server"),
            RustyError::ImmediateWebSocketClose =>
                write!(f, "web socket sent an unexpected close message"),
                RustyError::UnexpectedMessage =>
                write!(f, "got unexpected message from web socket"),
        }
    }
}

impl Error for RustyError { }
