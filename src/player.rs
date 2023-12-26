//!

#![allow(dead_code)] // TODO: remove this.

use std::sync::Arc;

use crate::utils::InnerArc;

/// TODO:
pub struct PlayerRef {
}

/// TODO:
#[derive(Clone)]
pub struct Player {
    inner: Arc<PlayerRef>,
}

impl InnerArc for Player {
    type Ref = PlayerRef;

    fn instance(&self) -> &Arc<Self::Ref> {
        &self.inner
    }
}
