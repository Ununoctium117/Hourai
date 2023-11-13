use crate::{prelude::*, AppState};
use actix_web::{http::StatusCode, web};
use hourai::{models::id::Id, proto::auto_config::*, proto::guild_configs::*};
use hourai_redis::CachedGuildConfig;

async fn get_config<T>(data: web::Data<AppState>, path: web::Path<u64>) -> Result<Option<Vec<u8>>>
where
    T: protobuf::MessageFull + CachedGuildConfig,
{
    // TODO(james7132): Properly set up authN and authZ for this
    let proto = data
        .redis
        .guild(Id::new(path.into_inner()))
        .configs()
        .fetch::<T>()
        .await
        .http_error(StatusCode::NOT_FOUND, "Guild not found")?;

    Ok(proto
        .map(|t| protobuf_json_mapping::print_to_string(&t))
        .transpose()
        .http_internal_error("Failed to serialize config")?
        .map(String::into_bytes))
}

fn add_config<T: protobuf::MessageFull + CachedGuildConfig>(
    cfg: &mut web::ServiceConfig,
    endpoint: &str,
) {
    cfg.service(
        web::resource(format!("/{{guild_id}}/{}", endpoint).as_str())
            .route(web::get().to(get_config::<T>)),
    );
}

pub fn scoped_config(cfg: &mut web::ServiceConfig) {
    add_config::<AnnouncementConfig>(cfg, "announce");
    add_config::<AutoConfig>(cfg, "auto");
    add_config::<ModerationConfig>(cfg, "moderation");
    add_config::<LoggingConfig>(cfg, "logging");
    add_config::<RoleConfig>(cfg, "roles");
    add_config::<VerificationConfig>(cfg, "validation");
}
