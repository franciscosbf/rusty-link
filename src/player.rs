//!

#![allow(dead_code)] // TODO: remove this.

use std::{ops::Deref, sync::Arc};

use crate::node::{GuildId, Node};

/// TODO:
pub struct PlayerRef {
    guild_id: GuildId,
    node: Node,
}

impl PlayerRef {
    /// Returns the guild where the player is running in.
    pub fn guild(&self) -> &GuildId {
        &self.guild_id
    }

    /// Returns the associated node.
    pub fn node(&self) -> &Node {
        &self.node
    }
}

/// Holds the original player so you can use as if it was protected by an [`Arc`] instance.
#[derive(Clone)]
pub struct Player {
    inner: Arc<PlayerRef>,
}

impl Player {
    pub(crate) fn new(guild: GuildId, node: Node) -> Self {
        let inner = PlayerRef {
            guild_id: guild,
            node,
        };
        Self {
            inner: Arc::new(inner),
        }
    }
}

impl Deref for Player {
    type Target = Arc<PlayerRef>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
