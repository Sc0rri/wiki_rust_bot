use crate::ai::AiService;
use crate::dedup::DedupService;
use crate::github::GitHubService;
use crate::parser::ParserService;
use crate::state::{ContentStatus, PendingItem, TextTransition, UserState};
use crate::telegram::{self, TelegramService, Update};
use crate::{get_env_or_secret, log_event};
use worker::*;

const STATE_TTL_SECONDS: u64 = 600;

pub async fn handle_update(env: Env, ctx: Context, update_raw: String) -> Result<()> {
    let update: Update = match serde_json::from_str(&update_raw) {
        Ok(update) => update,
        Err(err) => {
            crate::log_event!("warn", "telegram.update.invalid_json", "error={}", err);
            return Ok(());
        }
    };

    let allowed_username = get_env_or_secret(&env, "ALLOWED_USERNAME", "");
    if allowed_username.is_empty() {
        crate::log_event!("error", "config.allowed_username_missing");
        return Ok(());
    }

    if let Some(cq) = update.callback_query {
        if !username_is_allowed(cq.from.username.as_ref(), &allowed_username) {
            crate::log_event!(
                "warn",
                "telegram.access_denied",
                "kind=callback user_id={}",
                cq.from.id
            );
            return Ok(());
        }

        let env_clone = env.clone();
        ctx.wait_until(async move {
            if let Err(e) = handle_callback_query(env_clone, cq).await {
                crate::log_event!("error", "telegram.callback.failed", "error={:?}", e);
            }
        });
        return Ok(());
    }

    if let Some(msg) = update.message {
        let sender = msg.from.as_ref();
        if !username_is_allowed(sender.and_then(|u| u.username.as_ref()), &allowed_username) {
            let user_id = sender.map(|u| u.id).unwrap_or_default();
            crate::log_event!(
                "warn",
                "telegram.access_denied",
                "kind=message user_id={}",
                user_id
            );
            return Ok(());
        }

        let chat_id = msg.chat.id;
        let text = msg.text.clone().unwrap_or_default().trim().to_string();
        
        if text.is_empty() {
            crate::log_event!(
                "info",
                "telegram.message.ignored_empty",
                "chat_id={}",
                chat_id
            );
            return Ok(());
        }

        crate::log_event!(
            "info",
            "telegram.text.received",
            "chat_id={} text={}",
            chat_id,
            text.chars().count()
        );

        let env_clone = env.clone();
        ctx.wait_until(async move {
            if let Err(e) = handle_text(env_clone, chat_id, text).await {
                crate::log_event!("error", "telegram.text.failed", "error={:?}", e);
            }
        });
    }

    Ok(())
}

fn username_is_allowed(username: Option<&String>, allowed_username: &str) -> bool {
    username.map(|u| u.as_str()).unwrap_or_default() == allowed_username
}

async fn handle_text(env: Env, chat_id: i64, text: String) -> Result<()> {
    let bot_token = env.secret("BOT_TOKEN")?.to_string();
    let kv = env.kv("STATE_STORE")?;
    let dedup_kv = env.kv("DEDUP_STORE")?;
    let state_key = format!("{}_state", chat_id);

    let state = load_state(&kv, &state_key).await?;
    crate::log_event!(
        "info",
        "state.loaded",
        "chat_id={} state={}",
        chat_id,
        state_name(&state)
    );

    let transition = state.text_transition(&text);
    
    if transition == TextTransition::Cancel {
        delete_state(&kv, &state_key, chat_id).await?;
        TelegramService::send_message(
            &bot_token,
            chat_id,
            "❌ Cancelled.",
            Some(TelegramService::remove_keyboard()),
        )
        .await?;
        return Ok(());
    }

    match transition {
        TextTransition::Cancel => unreachable!(),
        TextTransition::SelectType(content_type) => {
            let content_type_clone = content_type.clone();
            let status_kb = TelegramService::status_keyboard(&content_type_clone);
            let state = UserState::AwaitingStatus {
                title: match &state {
                    UserState::AwaitingType { raw_text } => raw_text.clone(),
                    _ => text.clone(),
                },
                content_type,
            };
            save_state(&kv, &state_key, &state).await?;
            
            TelegramService::send_message(
                &bot_token,
                chat_id,
                &format!("{} {} — already consumed or in watchlist?", 
                    content_type_clone.emoji(), 
                    content_type_clone.label()),
                Some(status_kb),
            )
            .await?;
        }
        TextTransition::SelectStatus(status) => {
            if let UserState::AwaitingStatus { title, content_type } = &state {
                let item = PendingItem {
                    title: title.clone(),
                    content_type: content_type.clone(),
                    status,
                    url: None,
                    author: None,
                    year: None,
                    description: None,
                };
                
                let state = UserState::AwaitingDetails { item: item.clone() };
                save_state(&kv, &state_key, &state).await?;
                
                TelegramService::send_message(
                    &bot_token,
                    chat_id,
                    "Add details? (author, year) or skip:",
                    Some(TelegramService::details_keyboard()),
                )
                .await?;
            }
        }
        TextTransition::UpdateDetails { field, value } => {
            if let UserState::AwaitingDetails { mut item } = state.clone() {
                match field.as_str() {
                    "author" => item.author = Some(value),
                    "year" => {
                        if let Ok(year) = value.parse::<i32>() {
                            item.year = Some(year);
                        }
                    }
                    _ => {}
                }
                
                let state = UserState::AwaitingDetails { item: item.clone() };
                save_state(&kv, &state_key, &state).await?;
                
                TelegramService::send_message(
                    &bot_token,
                    chat_id,
                    "More details? or [Save]",
                    Some(TelegramService::details_keyboard()),
                )
                .await?;
            }
        }
        TextTransition::Confirm => {
            if let UserState::AwaitingDetails { item } = state {
                delete_state(&kv, &state_key, chat_id).await?;
                
                let dedup_key = DedupService::title_key(&item.title);
                if DedupService::is_processed(&dedup_kv, &dedup_key).await? {
                    TelegramService::send_message(
                        &bot_token,
                        chat_id,
                        "⚠️ Already saved.",
                        Some(TelegramService::remove_keyboard()),
                    )
                    .await?;
                    return Ok(());
                }
                
                TelegramService::send_message(
                    &bot_token,
                    chat_id,
                    "⏳ Saving...",
                    Some(TelegramService::remove_keyboard()),
                )
                .await?;
                
                match GitHubService::save_to_inbox(&env, &item).await {
                    Ok(path) => {
                        DedupService::mark_processed(&dedup_kv, &dedup_key).await?;
                        TelegramService::send_message(
                            &bot_token,
                            chat_id,
                            &format!("✅ Saved: {}", path),
                            Some(TelegramService::remove_keyboard()),
                        )
                        .await?;
                    }
                    Err(e) => {
                        TelegramService::send_message(
                            &bot_token,
                            chat_id,
                            &format!("❌ Error: {}", e),
                            Some(TelegramService::remove_keyboard()),
                        )
                        .await?;
                    }
                }
            }
        }
        TextTransition::ProcessFresh => {
            delete_state(&kv, &state_key, chat_id).await?;
            process_fresh_text(env, &bot_token, &kv, &dedup_kv, chat_id, &text)
                .await?;
        }
    }

    Ok(())
}

async fn process_fresh_text(
    env: Env,
    bot_token: &str,
    kv: &kv::KvStore,
    dedup_kv: &kv::KvStore,
    chat_id: i64,
    text: &str,
) -> Result<()> {
    if ParserService::is_url(text) {
        let dedup_key = DedupService::url_key(text);
        if DedupService::is_processed(dedup_kv, &dedup_key).await? {
            TelegramService::send_message(
                bot_token,
                chat_id,
                "⚠️ Already processed.",
                Some(TelegramService::remove_keyboard()),
            )
            .await?;
            return Ok(());
        }

        TelegramService::send_message(
            bot_token,
            chat_id,
            "⏳ Processing link...",
            Some(TelegramService::remove_keyboard()),
        )
        .await?;

        let item = match AiService::analyze_url(&env, text, "").await {
            Ok(Some(item)) => item,
            _ => {
                TelegramService::send_message(
                    bot_token,
                    chat_id,
                    "❌ Could not analyze link. Try sending the title instead.",
                    Some(TelegramService::remove_keyboard()),
                )
                .await?;
                return Ok(());
            }
        };

        let dedup_key = DedupService::title_key(&item.title);
        if DedupService::is_processed(dedup_kv, &dedup_key).await? {
            TelegramService::send_message(
                bot_token,
                chat_id,
                "⚠️ Already saved.",
                Some(TelegramService::remove_keyboard()),
            )
            .await?;
            return Ok(());
        }

        match GitHubService::save_to_inbox(&env, &item).await {
            Ok(path) => {
                DedupService::mark_processed(dedup_kv, &DedupService::url_key(text)).await?;
                DedupService::mark_processed(dedup_kv, &dedup_key).await?;
                TelegramService::send_message(
                    bot_token,
                    chat_id,
                    &format!("✅ Saved: {}", path),
                    Some(TelegramService::remove_keyboard()),
                )
                .await?;
            }
            Err(e) => {
                TelegramService::send_message(
                    bot_token,
                    chat_id,
                    &format!("❌ Error: {}", e),
                    Some(TelegramService::remove_keyboard()),
                )
                .await?;
            }
        }
    } else {
        let dedup_key = DedupService::title_key(text);
        if DedupService::is_processed(dedup_kv, &dedup_key).await? {
            TelegramService::send_message(
                bot_token,
                chat_id,
                "⚠️ Already saved.",
                Some(TelegramService::remove_keyboard()),
            )
            .await?;
            return Ok(());
        }

        let item = match AiService::analyze_content(&env, text).await {
            Ok(Some(item)) => item,
            Ok(None) => {
                let state = UserState::AwaitingType {
                    raw_text: text.to_string(),
                };
                save_state(kv, &format!("{}_state", chat_id), &state).await?;
                
                TelegramService::send_message(
                    bot_token,
                    chat_id,
                    "What type is this?",
                    Some(TelegramService::type_keyboard()),
                )
                .await?;
                return Ok(());
            }
            Err(e) => {
                TelegramService::send_message(
                    bot_token,
                    chat_id,
                    &format!("❌ AI error: {}", e),
                    Some(TelegramService::remove_keyboard()),
                )
                .await?;
                return Ok(());
            }
        };

        let state = UserState::AwaitingStatus {
            title: item.title.clone(),
            content_type: item.content_type.clone(),
        };
        save_state(kv, &format!("{}_state", chat_id), &state).await?;
        
        let status_kb = TelegramService::status_keyboard(&item.content_type);
        TelegramService::send_message(
            bot_token,
            chat_id,
            &format!("{} {} — already consumed or in watchlist?", 
                item.content_type.emoji(), 
                item.content_type.label()),
            Some(status_kb),
        )
        .await?;
    }

    Ok(())
}

async fn handle_callback_query(env: Env, cq: telegram::CallbackQuery) -> Result<()> {
    let bot_token = env.secret("BOT_TOKEN")?.to_string();
    TelegramService::answer_callback_query(&bot_token, &cq.id, None).await?;

    let Some(message) = cq.message.as_ref() else {
        crate::log_event!("warn", "telegram.callback.missing_message");
        return Ok(());
    };

    let chat_id = message.chat.id;
    let data = cq.data.unwrap_or_default();
    let kv = env.kv("STATE_STORE")?;
    let state_key = format!("{}_state", chat_id);
    let state = load_state(&kv, &state_key).await?;

    crate::log_event!(
        "info",
        "telegram.callback.received",
        "chat_id={} data={}",
        chat_id,
        data
    );

    match data.as_str() {
        "confirm" => {
            if let UserState::AwaitingDetails { item } = state {
                delete_state(&kv, &state_key, chat_id).await?;
                let dedup_kv = env.kv("DEDUP_STORE")?;
                let dedup_key = DedupService::title_key(&item.title);
                
                if DedupService::is_processed(&dedup_kv, &dedup_key).await? {
                    TelegramService::send_message(
                        &bot_token,
                        chat_id,
                        "⚠️ Already saved.",
                        Some(TelegramService::remove_keyboard()),
                    )
                    .await?;
                    return Ok(());
                }
                
                TelegramService::send_message(
                    &bot_token,
                    chat_id,
                    "⏳ Saving...",
                    Some(TelegramService::remove_keyboard()),
                )
                .await?;
                
                match GitHubService::save_to_inbox(&env, &item).await {
                    Ok(path) => {
                        DedupService::mark_processed(&dedup_kv, &dedup_key).await?;
                        TelegramService::send_message(
                            &bot_token,
                            chat_id,
                            &format!("✅ Saved: {}", path),
                            Some(TelegramService::remove_keyboard()),
                        )
                        .await?;
                    }
                    Err(e) => {
                        TelegramService::send_message(
                            &bot_token,
                            chat_id,
                            &format!("❌ Error: {}", e),
                            Some(TelegramService::remove_keyboard()),
                        )
                        .await?;
                    }
                }
            }
        }
        "cancel" => {
            delete_state(&kv, &state_key, chat_id).await?;
            TelegramService::send_message(
                &bot_token,
                chat_id,
                "❌ Cancelled.",
                Some(TelegramService::remove_keyboard()),
            )
            .await?;
        }
        _ => {
            crate::log_event!(
                "warn",
                "telegram.callback.unknown",
                "data={}",
                data
            );
        }
    }

    Ok(())
}

fn state_name(state: &UserState) -> &'static str {
    match state {
        UserState::None => "none",
        UserState::AwaitingType { .. } => "awaiting_type",
        UserState::AwaitingStatus { .. } => "awaiting_status",
        UserState::AwaitingDetails { .. } => "awaiting_details",
        UserState::AwaitingConfirmation { .. } => "awaiting_confirmation",
    }
}

async fn load_state(kv: &kv::KvStore, state_key: &str) -> Result<UserState> {
    let Some(state_str) = kv.get(state_key).text().await? else {
        return Ok(UserState::None);
    };

    Ok(UserState::parse_or_none(&state_str))
}

async fn save_state(kv: &kv::KvStore, state_key: &str, state: &UserState) -> Result<()> {
    let state_json = serde_json::to_string(state)?;
    kv.put(state_key, &state_json)?
        .expiration_ttl(STATE_TTL_SECONDS)
        .execute()
        .await?;
    crate::log_event!("info", "state.saved", "state={}", state_name(state));
    Ok(())
}

async fn delete_state(kv: &kv::KvStore, state_key: &str, chat_id: i64) -> Result<()> {
    kv.delete(state_key).await?;
    crate::log_event!("info", "state.deleted", "chat_id={}", chat_id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_name_should_return_correct_names() {
        assert_eq!(state_name(&UserState::None), "none");
        assert_eq!(
            state_name(&UserState::AwaitingType {
                raw_text: "test".to_string()
            }),
            "awaiting_type"
        );
    }
}