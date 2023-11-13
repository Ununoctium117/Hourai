#[macro_use]
extern crate delegate;

pub mod actions;
pub mod escalation;
mod storage;

pub use storage::Storage;

use anyhow::Result;
use hourai::{
    interactions::InteractionError,
    models::{
        guild::{Guild, Permissions, Role},
        id::{
            marker::{GuildMarker, RoleMarker},
            Id,
        },
    },
    proto::cache::CachedRoleProto,
};
use hourai_redis::RedisClient;
use hourai_sql::{Member, SqlPool};
use rand::Rng;
use std::collections::HashSet;

pub fn is_moderator_role(role: &CachedRoleProto) -> bool {
    let name = role.name().to_lowercase();
    let perms = Permissions::from_bits_truncate(role.permissions());
    perms.contains(Permissions::ADMINISTRATOR)
        || name.starts_with("mod")
        || name.starts_with("admin")
}

pub async fn find_moderator_roles(
    guild_id: Id<GuildMarker>,
    redis: &RedisClient,
) -> Result<Vec<CachedRoleProto>> {
    Ok(redis
        .guild(guild_id)
        .fetch_all_resources::<Role>()
        .await?
        .into_values()
        .filter(is_moderator_role)
        .collect())
}

pub async fn find_moderators(
    guild_id: Id<GuildMarker>,
    sql: &SqlPool,
    redis: &RedisClient,
) -> Result<Vec<Member>> {
    let mod_roles: Vec<Id<RoleMarker>> = find_moderator_roles(guild_id, redis)
        .await?
        .into_iter()
        .map(|role| Id::new(role.role_id()))
        .collect();
    Ok(Member::find_with_roles(guild_id, mod_roles)
        .fetch_all(sql)
        .await?
        .into_iter()
        .filter(|moderator| !moderator.bot)
        .collect())
}

pub async fn find_online_moderators(
    guild_id: Id<GuildMarker>,
    sql: &SqlPool,
    redis: &RedisClient,
) -> Result<Vec<Member>> {
    let mods = find_moderators(guild_id, sql, redis).await?;
    let online = redis
        .online_status()
        .find_online(guild_id, mods.iter().map(|member| member.user_id()))
        .await?;
    Ok(mods
        .into_iter()
        .filter(|member| online.contains(&member.user_id()))
        .collect())
}

pub async fn ping_online_mod(
    guild_id: Id<GuildMarker>,
    storage: &Storage,
) -> Result<(String, String)> {
    let online_mods = find_online_moderators(guild_id, storage.sql(), storage.redis()).await?;
    let guild = storage
        .redis()
        .guild(guild_id)
        .fetch_resource::<Guild>(guild_id)
        .await?
        .ok_or(InteractionError::NotInGuild)?;

    let mention: String;
    let ping: String;
    if online_mods.is_empty() {
        mention = format!("<@{}>", guild.owner_id());
        ping = format!("<@{}>, No mods online!", guild.owner_id());
    } else {
        let idx = rand::thread_rng().gen_range(0..online_mods.len());
        mention = format!("<@{}>", online_mods[idx].user_id());
        ping = mention.clone();
    };

    Ok((mention, ping))
}

pub async fn is_moderator(
    guild_id: Id<GuildMarker>,
    mut roles: impl Iterator<Item = Id<RoleMarker>>,
    redis: &RedisClient,
) -> Result<bool> {
    let moderator_roles: HashSet<Id<RoleMarker>> = find_moderator_roles(guild_id, redis)
        .await?
        .iter()
        .map(|role| Id::new(role.role_id()))
        .collect();
    Ok(roles.any(move |role_id| moderator_roles.contains(&role_id)))
}
