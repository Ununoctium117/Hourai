use crate::config::HouraiConfig;
use metrics_exporter_prometheus::PrometheusBuilder;
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Mutex;
use std::{convert::TryFrom, future::Future, pin::Pin, sync::Arc};
use tracing::debug;

pub fn start_logging() {
    tracing_subscriber::fmt()
        .with_level(true)
        .with_thread_ids(true)
        .with_timer(tracing_subscriber::fmt::time::UtcTime::rfc_3339())
        .with_env_filter(tracing_subscriber::filter::EnvFilter::from_default_env())
        .compact()
        .init();
}

pub fn init(config: &HouraiConfig) {
    start_logging();
    debug!("Loaded Config: {:?}", config);

    debug!("Loaded Config: {:?}", config);

    let metrics_port = config.metrics.port.unwrap_or(9090);
    let socket = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), metrics_port);
    PrometheusBuilder::new()
        .with_http_listener(socket)
        .install()
        .expect("Failed to set up Prometheus metrics exporter");

    debug!(
        "Metrics endpoint listening on http://0.0.0.0:{}",
        metrics_port
    );
}

pub fn load_gateway_config(
    config: &HouraiConfig,
    intents: twilight_gateway::Intents,
) -> twilight_gateway::Config {
    let mut builder =
        twilight_gateway::ConfigBuilder::new(config.discord.bot_token.clone(), intents);
    if let Some(ref uri) = config.discord.gateway_queue {
        builder = builder.queue(Arc::new(GatewayQueue(
            hyper::Uri::try_from(uri.clone()).unwrap(),
        )));
    }

    builder.build()
}

pub type ShardMessageSenders = Arc<Vec<twilight_gateway::MessageSender>>;

pub async fn create_shards(
    config: &HouraiConfig,
    http_client: &twilight_http::Client,
    intents: twilight_gateway::Intents,
    events: twilight_gateway::EventTypeFlags,
    resume_sessions: HashMap<u64, twilight_gateway::Session>,
) -> Result<(Vec<twilight_gateway::Shard>, ShardMessageSenders), anyhow::Error> {
    let gateway_config = load_gateway_config(config, intents);
    let sessions = Mutex::new(resume_sessions);

    let shards: Vec<_> = twilight_gateway::stream::create_recommended(
        http_client,
        gateway_config,
        |id, mut builder| {
            builder = builder.event_types(events);

            if let Some(session) = sessions.lock().unwrap().remove(&id.number()) {
                builder = builder.session(session);
            }

            builder.build()
        },
    )
    .await
    .expect("Failed to connect to the discord gateway")
    .collect();

    let shard_message_senders = Arc::new(shards.iter().map(|shard| shard.sender()).collect());

    Ok((shards, shard_message_senders))
}

pub fn http_client(config: &HouraiConfig) -> twilight_http::Client {
    debug!("Creating Discord HTTP client");
    // Use the twilight HTTP proxy when configured
    if let Some(proxy) = config.discord.proxy.as_ref() {
        twilight_http::Client::builder()
            .token(config.discord.bot_token.clone())
            .proxy(proxy.clone(), true)
            .ratelimiter(None)
            .build()
    } else {
        twilight_http::Client::builder()
            .token(config.discord.bot_token.clone())
            .build()
    }
}

#[derive(Debug, Clone)]
pub struct GatewayQueue(hyper::Uri);

impl twilight_gateway::queue::Queue for GatewayQueue {
    fn request<'a>(&'a self, _: [u64; 2]) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        tracing::debug!("Queueing to IDENTIFY with the gateway...");
        Box::pin(async move {
            if let Err(err) = hyper::Client::new().get(self.0.clone()).await {
                tracing::error!("Error while querying the shared gateway queue: {}", err);
            } else {
                tracing::debug!("Finished waiting to re-IDENTIFY with the Discord gateway.");
            }
        })
    }
}
