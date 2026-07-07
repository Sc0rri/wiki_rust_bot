use crate::dedup::DedupService;
use crate::detector::Detector;
use crate::github::GitHubService;
use crate::parser::ParserService;
use crate::state::{ContentStatus, KnowledgeType, PendingItem, TextTransition, UserState};
use crate::telegram::{TelegramService, Update};
use crate::{get_env_or_secret, log_event};
use worker::*;

const STATE_TTL_SECONDS: u64 = 600;

pub async fn handle_update(env: Env, ctx: Context, update_raw: String) -> Result<()> {
    let update: Update = match serde_json::from_str(&update_raw) {
        Ok(update) => update,
        Err(err) => {
            log_event!("warn", "telegram.update.invalid_json", "error={}", err);
            return Ok(());
        }
    };

    let allowed_username = get_env_or_secret(&env, "ALLOWED_USERNAME", "");
    if allowed_username.is_empty() {
        log_event!("error", "config.allowed_username_missing");
        return Ok(());
    }

    if let Some(msg) = update.message {
        let sender = msg.from.as_ref();
        if !username_is_allowed(sender.and_then(|u| u.username.as_ref()), &allowed_username) {
            return Ok(());
        }

        let chat_id = msg.chat.id;

        if let Some(photos) = &msg.photo {
            if !photos.is_empty() {
                let env_clone = env.clone();
                ctx.wait_until(async move {
                    if let Err(e) = handle_media(env_clone, chat_id, "image").await {
                        log_event!("error", "telegram.photo.failed", "error={:?}", e);
                    }
                });
                return Ok(());
            }
        }

        if let Some(doc) = msg.document.as_ref().cloned() {
            let env_clone = env.clone();
            ctx.wait_until(async move {
                let file_name = doc.file_name.unwrap_or_default();
                if file_name.to_lowercase().ends_with(".pdf") {
                    if let Err(e) = handle_media(env_clone, chat_id, "pdf").await {
                        log_event!("error", "telegram.pdf.failed", "error={:?}", e);
                    }
                }
            });
            return Ok(());
        }

        let text = msg.text.clone().unwrap_or_default().trim().to_string();
        if text.is_empty() {
            return Ok(());
        }

        log_event!("info", "telegram.text.received", "chat_id={} text={}", chat_id, text.chars().count());
        let env_clone = env.clone();
        ctx.wait_until(async move {
            if let Err(e) = handle_text(env_clone, chat_id, text).await {
                log_event!("error", "telegram.text.failed", "error={:?}", e);
            }
        });
    }

    Ok(())
}

fn username_is_allowed(username: Option<&String>, allowed: &str) -> bool {
    username.map(|u| u.as_str()).unwrap_or_default() == allowed
}

async fn handle_media(env: Env, chat_id: i64, media_type: &str) -> Result<()> {
    let bot_token = env.secret("BOT_TOKEN")?.to_string();
    let text = match media_type {
        "image" => "🖼 Image received\n\nWhat type?",
        "pdf" => "📕 PDF received\n\nWhat type?",
        _ => return Ok(()),
    };
    TelegramService::send_message(&bot_token, chat_id, text, Some(TelegramService::type_keyboard())).await?;
    let kv = env.kv("STATE_STORE")?;
    let state = UserState::AwaitingType { raw_text: media_type.to_string(), detected: None };
    save_state(&kv, &format!("{}_state", chat_id), &state).await?;
    Ok(())
}

async fn handle_text(env: Env, chat_id: i64, text: String) -> Result<()> {
    let bot_token = env.secret("BOT_TOKEN")?.to_string();
    let kv = env.kv("STATE_STORE")?;
    let dedup_kv = env.kv("DEDUP_STORE")?;
    let state_key = format!("{}_state", chat_id);

    let state = load_state(&kv, &state_key).await?;
    let transition = state.text_transition(&text);

    if transition == TextTransition::Cancel {
        delete_state(&kv, &state_key, chat_id).await?;
        TelegramService::send_message(&bot_token, chat_id, "❌ Cancelled.", Some(TelegramService::remove_keyboard())).await?;
        return Ok(());
    }

    match transition {
        TextTransition::Cancel => unreachable!(),
        TextTransition::SelectType(kt) => {
            let raw_text = match &state {
                UserState::AwaitingType { raw_text, .. } => raw_text.clone(),
                _ => text.clone(),
            };
            let detected = match &state {
                UserState::AwaitingType { detected, .. } => detected.clone(),
                _ => None,
            };
            let mut item = PendingItem::new(raw_text, kt.clone());
            item.source = "telegram".to_string();
            if let Some(d) = detected { item.provider = d.provider; item.url = Some(d.url); item.description = d.description; }

            if kt.has_categories() {
                let state = UserState::AwaitingCategory { item };
                save_state(&kv, &state_key, &state).await?;
                TelegramService::send_message(&bot_token, chat_id, "📄 Category?", Some(category_keyboard())).await?;
            } else if kt.has_status_options() {
                let status_kb = TelegramService::status_keyboard(&kt);
                let state = UserState::AwaitingStatus { item };
                save_state(&kv, &state_key, &state).await?;
                TelegramService::send_message(&bot_token, chat_id, &format!("{} Status?", kt.emoji()), Some(status_kb)).await?;
            } else {
                save_and_finish(env, &bot_token, &dedup_kv, chat_id, item).await?;
            }
        }
        TextTransition::SelectCategory(category) => {
            if let UserState::AwaitingCategory { mut item } = state {
                item.category = Some(category);
                let kt = item.knowledge_type.clone();
                let status_kb = TelegramService::status_keyboard(&kt);
                let state = UserState::AwaitingStatus { item };
                save_state(&kv, &state_key, &state).await?;
                TelegramService::send_message(&bot_token, chat_id, &format!("{} Status?", kt.emoji()), Some(status_kb)).await?;
            }
        }
        TextTransition::SelectStatus(status) => {
            if let UserState::AwaitingStatus { mut item } = state {
                item.status = status;
                let state = UserState::AwaitingRating { item };
                save_state(&kv, &state_key, &state).await?;
                TelegramService::send_message(&bot_token, chat_id, "Rate 1-10 or skip:", Some(TelegramService::remove_keyboard())).await?;
            }
        }
        TextTransition::SetRating(rating) => {
            if let UserState::AwaitingRating { mut item } = state {
                item.rating = Some(rating);
                let state = UserState::AwaitingComment { item };
                save_state(&kv, &state_key, &state).await?;
                TelegramService::send_message(&bot_token, chat_id, "Add a comment or skip:", Some(TelegramService::remove_keyboard())).await?;
            }
        }
        TextTransition::SetComment(comment) => {
            if let UserState::AwaitingComment { mut item } = state {
                item.comment = Some(comment);
                let preview = build_preview(&item);
                TelegramService::send_message(&bot_token, chat_id, &preview, Some(TelegramService::confirm_keyboard())).await?;
                let state = UserState::AwaitingConfirmation { item };
                save_state(&kv, &state_key, &state).await?;
            }
        }
        TextTransition::Confirm => {
            if let UserState::AwaitingConfirmation { item } = state {
                delete_state(&kv, &state_key, chat_id).await?;
                save_and_finish(env, &bot_token, &dedup_kv, chat_id, item).await?;
            } else if let UserState::AwaitingRating { item } = state {
                delete_state(&kv, &state_key, chat_id).await?;
                let preview = build_preview(&item);
                TelegramService::send_message(&bot_token, chat_id, &preview, Some(TelegramService::confirm_keyboard())).await?;
                let state = UserState::AwaitingConfirmation { item };
                save_state(&kv, &state_key, &state).await?;
            } else if let UserState::AwaitingComment { item } = state {
                delete_state(&kv, &state_key, chat_id).await?;
                let preview = build_preview(&item);
                TelegramService::send_message(&bot_token, chat_id, &preview, Some(TelegramService::confirm_keyboard())).await?;
                let state = UserState::AwaitingConfirmation { item };
                save_state(&kv, &state_key, &state).await?;
            }
        }
        TextTransition::ProcessFresh => {
            delete_state(&kv, &state_key, chat_id).await?;
            process_fresh(env, &bot_token, &dedup_kv, chat_id, &text).await?;
        }
    }

    Ok(())
}

async fn process_fresh(env: Env, bot_token: &str, _dedup_kv: &worker::kv::KvStore, chat_id: i64, text: &str) -> Result<()> {
    if ParserService::is_url(text) {
        let detected = Detector::detect(text);
        TelegramService::send_message(bot_token, chat_id, &detected.preview_text(), Some(TelegramService::type_keyboard())).await?;
        let kv = env.kv("STATE_STORE")?;
        let state = UserState::AwaitingType { raw_text: text.to_string(), detected: Some(detected) };
        save_state(&kv, &format!("{}_state", chat_id), &state).await?;
    } else {
        TelegramService::send_message(bot_token, chat_id, "Detected title.\n\nWhat type?", Some(TelegramService::type_keyboard())).await?;
        let kv = env.kv("STATE_STORE")?;
        let state = UserState::AwaitingType { raw_text: text.to_string(), detected: None };
        save_state(&kv, &format!("{}_state", chat_id), &state).await?;
    }
    Ok(())
}

async fn save_and_finish(env: Env, bot_token: &str, dedup_kv: &worker::kv::KvStore, chat_id: i64, item: PendingItem) -> Result<()> {
    let dedup_key = DedupService::title_key(&item.title);
    if DedupService::is_processed(dedup_kv, &dedup_key).await? {
        TelegramService::send_message(bot_token, chat_id, "⚠️ Already saved.", Some(TelegramService::remove_keyboard())).await?;
        return Ok(());
    }

    TelegramService::send_message(bot_token, chat_id, "⏳ Saving...", Some(TelegramService::remove_keyboard())).await?;

    match GitHubService::save_to_inbox(&env, &item).await {
        Ok(path) => {
            DedupService::mark_processed(dedup_kv, &dedup_key).await?;
            if let Some(ref url) = item.url { DedupService::mark_processed(dedup_kv, &DedupService::url_key(url)).await?; }
            TelegramService::send_message(bot_token, chat_id, &format!("✅ Saved:\n{}", path), Some(TelegramService::remove_keyboard())).await?;
        }
        Err(e) => TelegramService::send_message(bot_token, chat_id, &format!("❌ Error: {}", e), Some(TelegramService::remove_keyboard())).await?,
    }
    Ok(())
}

fn build_preview(item: &PendingItem) -> String {
    let mut preview = format!("{} {}\n", item.knowledge_type.emoji(), item.title);
    if let Some(ref url) = item.url { preview.push_str(&format!("🔗 {}\n", url)); }
    if item.provider.label() != "" { preview.push_str(&format!("📦 {}\n", item.provider.label())); }
    preview.push_str(&format!("📌 Status: {}\n", item.status.label()));
    if let Some(r) = item.rating { preview.push_str(&format!("⭐ {}/10\n", r)); }
    if let Some(ref c) = item.comment { preview.push_str(&format!("💬 \"{}\"\n", c)); }
    preview.push_str("\nSave?");
    preview
}

fn category_keyboard() -> serde_json::Value {
    serde_json::json!({
        "keyboard": [
            [{"text": "💻 Programming"}, {"text": "📰 News"}],
            [{"text": "🧠 Concept"}, {"text": "📚 Education"}],
            [{"text": "🎮 Gaming"}, {"text": "🎬 Entertainment"}],
            [{"text": "📋 Other"}, {"text": "❌ Cancel"}]
        ],
        "one_time_keyboard": true, "resize_keyboard": true
    })
}

async fn load_state(kv: &worker::kv::KvStore, state_key: &str) -> Result<UserState> {
    let Some(s) = kv.get(state_key).text().await? else { return Ok(UserState::None); };
    Ok(UserState::parse_or_none(&s))
}

async fn save_state(kv: &worker::kv::KvStore, state_key: &str, state: &UserState) -> Result<()> {
    kv.put(state_key, &serde_json::to_string(state)?)?.expiration_ttl(STATE_TTL_SECONDS).execute().await?;
    Ok(())
}

async fn delete_state(kv: &worker::kv::KvStore, state_key: &str, chat_id: i64) -> Result<()> {
    kv.delete(state_key).await?;
    log_event!("info", "state.deleted", "chat_id={}", chat_id);
    Ok(())
}

fn state_name(state: &UserState) -> &'static str {
    match state {
        UserState::None => "none",
        UserState::AwaitingType { .. } => "awaiting_type",
        UserState::AwaitingCategory { .. } => "awaiting_category",
        UserState::AwaitingStatus { .. } => "awaiting_status",
        UserState::AwaitingRating { .. } => "awaiting_rating",
        UserState::AwaitingComment { .. } => "awaiting_comment",
        UserState::AwaitingConfirmation { .. } => "awaiting_confirmation",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_name_should_return_correct_names() {
        assert_eq!(state_name(&UserState::None), "none");
        assert_eq!(
            state_name(&UserState::AwaitingType { raw_text: "test".to_string(), detected: None }),
            "awaiting_type"
        );
    }
}