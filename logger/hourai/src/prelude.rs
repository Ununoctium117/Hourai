use crate::models::id::{marker::GuildMarker, Id};
pub use std::{sync::Arc, time::Duration};
pub use tracing::{debug, error, info, warn};
use twilight_gateway::{MessageSender, Shard};

pub trait ClusterExt {
    fn total_shards(&self) -> usize;

    /// Gets the shard ID for a guild.
    #[inline(always)]
    fn shard_id(&self, guild_id: Id<GuildMarker>) -> usize {
        ((guild_id.get() >> 22) % (self.total_shards() as u64)) as usize
    }
}
impl ClusterExt for Arc<Vec<MessageSender>> {
    fn total_shards(&self) -> usize {
        self.len()
    }
}
impl ClusterExt for Vec<Shard> {
    fn total_shards(&self) -> usize {
        self.len()
    }
}
