//!

#![allow(dead_code)] // TODO: remove this.

use std::sync::Arc;

use crate::utils::InnerArc;
use crate::node::{Node, GuildId};

/// TODO:
pub struct PlayerRef {
    guild_id: GuildId,
    node: Node,
}

/// Holds the original player so you can use as if it was protected by an [`Arc`] instance.
#[derive(Clone)]
pub struct Player {
    inner: Arc<PlayerRef>,
}

impl Player {
    pub(crate) fn new(guild_id: GuildId, node: Node) -> Self {
        let inner = PlayerRef { guild_id, node };
        Self { inner: Arc::new(inner) }
    }
}

impl InnerArc for Player {
    type Ref = PlayerRef;

    fn instance(&self) -> &Arc<Self::Ref> {
        &self.inner
    }
}
