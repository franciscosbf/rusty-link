use std::sync::Arc;

/// Implemented by structs that don't need to be wrapped by [`Arc`].
///
/// Internally, there's a reference that points to the internal implementation,
/// protected by [`Arc`].
pub trait InnerArc {
    /// Ref that points to the actual instance.
    type Ref;

    /// Returns the inner instance.
    fn instance(&self) -> &Arc<Self::Ref>;
}
