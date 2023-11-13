use crate::{
    http,
    interactions::{InteractionContext, InteractionError, InteractionResult},
    models::{
        application::interaction::{
            message_component::MessageComponentInteractionData, Interaction, InteractionData,
        },
        guild::PartialMember,
        id::{
            marker::{ApplicationMarker, ChannelMarker, GuildMarker, InteractionMarker},
            Id,
        },
        user::User,
    },
    proto::message_components::MessageComponentProto,
};
use anyhow::Result;
use base64::{Engine as _, engine::general_purpose as b64};
use protobuf::Message;
use std::marker::PhantomData;
use std::sync::Arc;

pub fn proto_to_custom_id(proto: &impl Message) -> Result<String> {
    Ok(b64::STANDARD.encode(proto.write_to_bytes()?))
}

#[derive(Clone)]
pub struct ComponentContext {
    pub http: Arc<http::Client>,
    pub component: Interaction,
    marker_: PhantomData<()>,
}

impl ComponentContext {
    pub fn new(client: Arc<http::Client>, interaction: Interaction) -> Self {
        assert!(matches!(
            interaction.data,
            Some(InteractionData::MessageComponent(_))
        ));
        Self {
            http: client,
            component: interaction,
            marker_: PhantomData,
        }
    }

    fn data(&self) -> &MessageComponentInteractionData {
        match &self.component.data {
            Some(InteractionData::MessageComponent(data)) => data,
            _ => panic!("Provided interaction data is not a message component"),
        }
    }

    pub fn metadata(&self) -> Result<MessageComponentProto> {
        let decoded = base64::engine::general_purpose::STANDARD.decode(&self.data().custom_id)?;
        Ok(MessageComponentProto::parse_from_bytes(&decoded)?)
    }
}

impl InteractionContext for ComponentContext {
    fn http(&self) -> &Arc<http::Client> {
        &self.http
    }

    fn id(&self) -> Id<InteractionMarker> {
        self.component.id
    }

    fn application_id(&self) -> Id<ApplicationMarker> {
        self.component.application_id
    }

    fn token(&self) -> &str {
        &self.component.token
    }

    fn guild_id(&self) -> InteractionResult<Id<GuildMarker>> {
        self.component.guild_id.ok_or(InteractionError::NotInGuild)
    }

    fn channel_id(&self) -> Id<ChannelMarker> {
        self.component.channel.as_ref().unwrap().id
    }

    fn member(&self) -> Option<&PartialMember> {
        self.component.member.as_ref()
    }

    fn user(&self) -> &User {
        let member = self
            .component
            .member
            .as_ref()
            .and_then(|member| member.user.as_ref());
        let user = self.component.user.as_ref();
        user.or(member).unwrap()
    }
}
