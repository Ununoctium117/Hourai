use anyhow::Result;
use futures::{channel::mpsc, stream::StreamExt};
use hourai::{
    models::{
        gateway::payload::{
            incoming::MemberChunk, outgoing::request_guild_members::RequestGuildMembers,
        },
        id::{marker::GuildMarker, Id},
    },
    prelude::*,
    ShardMessageSenders,
};
use std::collections::VecDeque;

enum Message {
    Guild(Id<GuildMarker>),
    GuildChunk {
        guild_id: Id<GuildMarker>,
        chunk_count: u32,
        chunk_index: u32,
    },
}

#[derive(Clone)]
pub struct MemberChunker(mpsc::UnboundedSender<Message>);

impl MemberChunker {
    pub fn new(shards: ShardMessageSenders) -> Self {
        let (tx, rx) = mpsc::unbounded();
        tokio::spawn(Self::run(rx, shards));
        Self(tx)
    }

    pub fn push_guild(&self, guild_id: Id<GuildMarker>) {
        self.0.unbounded_send(Message::Guild(guild_id)).ok();
    }

    pub fn push_chunk(&self, chunk: &MemberChunk) {
        self.0
            .unbounded_send(Message::GuildChunk {
                guild_id: chunk.guild_id,
                chunk_count: chunk.chunk_count,
                chunk_index: chunk.chunk_index,
            })
            .ok();
    }

    async fn run(mut rx: mpsc::UnboundedReceiver<Message>, shards: ShardMessageSenders) {
        let mut count = 0;
        let mut current = None;
        let mut queue = VecDeque::new();
        while let Some(msg) = rx.next().await {
            let chunk = match msg {
                Message::Guild(guild_id) => {
                    if current.is_some() {
                        if !queue.contains(&guild_id) {
                            queue.push_back(guild_id);
                        }
                        current
                    } else {
                        Some(guild_id)
                    }
                }
                Message::GuildChunk {
                    guild_id,
                    chunk_count,
                    chunk_index,
                } => {
                    tracing::debug!(
                        "Recieved chunk {} of {} for guild {}",
                        chunk_index,
                        chunk_count,
                        guild_id
                    );
                    count += 1;
                    if chunk_count == count {
                        count = 0;
                        queue.pop_front()
                    } else {
                        current
                    }
                }
            };

            if current == chunk {
                continue;
            }

            if let Some(ref guild) = chunk {
                Self::chunk_guild(&shards, *guild).await.unwrap();
            }

            current = chunk;
        }
    }

    async fn chunk_guild(
        shard_senders: &ShardMessageSenders,
        guild_id: Id<GuildMarker>,
    ) -> Result<()> {
        tracing::debug!("Chunking guild: {}", guild_id);
        let request = RequestGuildMembers::builder(guild_id)
            .presences(true)
            .query(String::new(), None);
        shard_senders[shard_senders.shard_id(guild_id)].command(&request)?;
        Ok(())
    }
}
