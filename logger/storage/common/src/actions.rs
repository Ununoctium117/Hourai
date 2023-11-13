use crate::{escalation::EscalationManager, Storage};
use anyhow::Result;
use chrono::{Duration, Utc};
use futures::future::{BoxFuture, FutureExt};
use hourai::{
    http::{self, request::AuditLogReason},
    models::{
        id::{marker::*, Id},
        user::User,
    },
    proto::action::*,
};
use hourai_sql::{Member, PendingAction};
use std::{collections::HashSet, sync::Arc};

const SECONDS_IN_DAY: u32 = 24 * 60 * 60;

#[derive(Clone)]
pub struct ActionExecutor {
    current_user: User,
    http: Arc<http::Client>,
    storage: Storage,
}

impl ActionExecutor {
    pub fn new(current_user: User, http: Arc<http::Client>, storage: Storage) -> Self {
        Self {
            current_user,
            http,
            storage,
        }
    }

    #[inline(always)]
    pub fn current_user(&self) -> &User {
        &self.current_user
    }

    #[inline(always)]
    pub fn http(&self) -> &Arc<http::Client> {
        &self.http
    }

    #[inline(always)]
    pub fn storage(&self) -> &Storage {
        &self.storage
    }

    pub async fn execute_action(&self, action: &Action) -> Result<()> {
        match action.details {
            Some(action::Details::Kick(_)) => self.execute_kick(action).await?,
            Some(action::Details::Ban(ref info)) => self.execute_ban(action, info).await?,
            Some(action::Details::Escalate(ref info)) => {
                self.execute_escalate(action, info).await?
            }
            Some(action::Details::Mute(ref info)) => self.execute_mute(action, info).await?,
            Some(action::Details::Deafen(ref info)) => self.execute_deafen(action, info).await?,
            Some(action::Details::ChangeRole(ref info)) => {
                self.execute_change_role(action, info).await?
            }
            Some(action::Details::DirectMessage(ref info)) => {
                if let Err(err) = self.execute_direct_message(action, info).await {
                    tracing::error!(
                        "Error while sending a message to a given channel for an action: {}",
                        err
                    );
                }
            }
            Some(action::Details::SendMessage(ref info)) => {
                if let Err(err) = self.execute_send_message(info).await {
                    tracing::error!(
                        "Error while sending a message to a given channel for an action: {}",
                        err
                    );
                }
            }
            Some(action::Details::DeleteMessages(ref info)) => {
                if let Err(err) = self.execute_delete_messages(info).await {
                    tracing::error!(
                        "Error while deleteing a message to a given channel for an action: {}",
                        err
                    );
                }
            }
            Some(_) | None => panic!("Cannot run action without a specified type"),
        };

        // Schedule undo if a duration is set
        if action.has_duration() {
            let timestamp = Utc::now() + Duration::seconds(action.duration() as i64);
            let mut undo = action.clone();
            Self::invert_action(&mut undo);
            undo.clear_duration();
            PendingAction::schedule(undo, timestamp)
                .execute(self.storage().sql())
                .await?;
        }
        Ok(())
    }

    fn invert_action(action: &mut Action) {
        match &mut action.details {
            Some(action::Details::Ban(ref mut info)) => {
                info.set_type(match info.type_() {
                    ban_member::Type::BAN => ban_member::Type::UNBAN,
                    ban_member::Type::UNBAN => ban_member::Type::BAN,
                    ban_member::Type::SOFTBAN => panic!("Cannot invert a softban"),
                });
            }
            Some(action::Details::Escalate(ref mut info)) => {
                info.set_amount(-info.amount());
            }
            Some(action::Details::Mute(ref mut info)) => {
                info.set_type(Self::invert_status(info.type_()));
            }
            Some(action::Details::Deafen(ref mut info)) => {
                info.set_type(Self::invert_status(info.type_()));
            }
            Some(action::Details::ChangeRole(ref mut info)) => {
                info.set_type(Self::invert_status(info.type_()));
            }
            Some(_) => {
                panic!("Cannot invert action: {:?}", action);
            }
            None => panic!("Cannot invert action without a specified type"),
        }
    }

    fn invert_status(status: StatusType) -> StatusType {
        match status {
            StatusType::APPLY => StatusType::UNAPPLY,
            StatusType::UNAPPLY => StatusType::APPLY,
            StatusType::TOGGLE => StatusType::TOGGLE,
        }
    }

    async fn execute_kick(&self, action: &Action) -> Result<()> {
        let guild_id = Id::new(action.guild_id());
        let user_id = Id::new(action.user_id());
        self.http
            .remove_guild_member(guild_id, user_id)
            .reason(action.reason())?
            .await?;
        Ok(())
    }

    async fn execute_ban(&self, action: &Action, info: &BanMember) -> Result<()> {
        let guild_id = Id::new(action.guild_id());
        let user_id = Id::new(action.user_id());
        if info.type_() != ban_member::Type::UNBAN {
            self.http
                .create_ban(guild_id, user_id)
                .reason(action.reason())?
                .delete_message_seconds(info.delete_message_days() * SECONDS_IN_DAY)?
                .await?;
        }
        if info.type_() != ban_member::Type::BAN {
            self.http
                .delete_ban(guild_id, user_id)
                .reason(action.reason())?
                .await?;
        }
        Ok(())
    }

    fn execute_escalate<'a>(
        &'a self,
        action: &'a Action,
        info: &'a EscalateMember,
    ) -> BoxFuture<'a, Result<()>> {
        async move {
            let guild_id = Id::new(action.guild_id());
            let user_id = Id::new(action.user_id());
            let manager = EscalationManager::new(self.clone());
            let guild = manager.guild(guild_id).await?;
            let history = guild.fetch_history(user_id).await?;
            history
                .apply_delta(
                    /*authorizer=*/ &self.current_user,
                    /*reason=*/ action.reason(),
                    /*diff=*/ info.amount(),
                    /*execute=*/ info.amount() >= 0,
                )
                .await?;
            Ok(())
        }
        .boxed()
    }

    async fn execute_mute(&self, action: &Action, info: &MuteMember) -> Result<()> {
        let guild_id = Id::new(action.guild_id());
        let user_id = Id::new(action.user_id());
        let mute = match info.type_() {
            StatusType::APPLY => true,
            StatusType::UNAPPLY => false,
            StatusType::TOGGLE => {
                !self
                    .http
                    .guild_member(guild_id, user_id)
                    .await?
                    .model()
                    .await?
                    .mute
            }
        };

        self.http
            .update_guild_member(guild_id, user_id)
            .mute(mute)
            .reason(action.reason())?
            .await?;

        Ok(())
    }

    async fn execute_deafen(&self, action: &Action, info: &DeafenMember) -> Result<()> {
        let guild_id = Id::new(action.guild_id());
        let user_id = Id::new(action.user_id());
        let deafen = match info.type_() {
            StatusType::APPLY => true,
            StatusType::UNAPPLY => false,
            StatusType::TOGGLE => {
                !self
                    .http
                    .guild_member(guild_id, user_id)
                    .await?
                    .model()
                    .await?
                    .deaf
            }
        };

        self.http
            .update_guild_member(guild_id, user_id)
            .deaf(deafen)
            .reason(action.reason())?
            .await?;

        Ok(())
    }

    async fn execute_change_role(&self, action: &Action, info: &ChangeRole) -> Result<()> {
        let guild_id = Id::new(action.guild_id());
        let user_id = Id::new(action.user_id());
        let member = Member::fetch(guild_id, user_id)
            .fetch_one(self.storage().sql())
            .await?;

        if info.role_ids.is_empty() {
            return Ok(());
        }

        let role_ids: HashSet<Id<RoleMarker>> =
            info.role_ids.iter().cloned().map(Id::new).collect();
        let mut roles: HashSet<Id<RoleMarker>> = member.role_ids().collect();
        match info.type_() {
            StatusType::APPLY => {
                roles.extend(role_ids);
            }
            StatusType::UNAPPLY => {
                roles.retain(|role_id| !role_ids.contains(role_id));
            }
            StatusType::TOGGLE => {
                for role_id in role_ids {
                    if roles.contains(&role_id) {
                        roles.remove(&role_id);
                    } else {
                        roles.insert(role_id);
                    }
                }
            }
        };

        let roles: Vec<Id<RoleMarker>> = roles.into_iter().collect();
        self.http
            .update_guild_member(guild_id, user_id)
            .roles(&roles)
            .reason(action.reason())?
            .await?;
        Ok(())
    }

    async fn execute_direct_message(&self, action: &Action, info: &DirectMessage) -> Result<()> {
        let user_id = Id::new(action.user_id());
        let channel = self
            .http
            .create_private_channel(user_id)
            .await?
            .model()
            .await?;

        self.http
            .create_message(channel.id)
            .content(info.content())?
            .await?;
        Ok(())
    }

    async fn execute_send_message(&self, info: &SendMessage) -> Result<()> {
        let channel_id = Id::new(info.channel_id());
        self.http
            .create_message(channel_id)
            .content(info.content())?
            .await?;
        Ok(())
    }

    async fn execute_delete_messages(&self, info: &DeleteMessages) -> Result<()> {
        let channel_id = Id::new(info.channel_id());
        let message_ids: Vec<Id<MessageMarker>> =
            info.message_ids.iter().cloned().map(Id::new).collect();
        match message_ids.len() {
            0 => return Ok(()),
            1 => {
                self.http.delete_message(channel_id, message_ids[0]).await?;
            }
            _ => {
                self.http.delete_messages(channel_id, &message_ids)?.await?;
            }
        }
        Ok(())
    }
}
