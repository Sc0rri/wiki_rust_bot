use crate::ai::AiService;
use crate::dedup::DedupService;
use crate::detector::Detector;
use crate::github::GitHubService;
use crate::parser::ParserService;
use crate::resolver::Resolver;
use crate::state::{PendingItem, ResourceProvider, TextTransition, UserState};
use crate::telegram::{TelegramService, Update};
use crate::{get_env_or_secret, log_event};
use worker::*;

const STATE_TTL_SECONDS: u64 = 1800; // 30 minutes

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
                // Take the largest photo (last in array)
                let file_id = photos.last().map(|p| p.file_id.clone()).unwrap_or_default();
                let env_clone = env.clone();
                ctx.wait_until(async move {
                    if let Err(e) = handle_media(env_clone, chat_id, "image", &file_id).await {
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
                let file_id = doc.file_id.clone();
                if file_name.to_lowercase().ends_with(".pdf") {
                    if let Err(e) = handle_media(env_clone, chat_id, "pdf", &file_id).await {
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

async fn handle_media(env: Env, chat_id: i64, media_type: &str, file_id: &str) -> Result<()> {
    let bot_token = env.secret("BOT_TOKEN")?.to_string();
    let text = match media_type {
        "image" => "🖼 Image received\n\nWhat type?",
        "pdf" => "📕 PDF received\n\nWhat type?",
        _ => return Ok(()),
    };
    TelegramService::send_message(&bot_token, chat_id, text, Some(TelegramService::type_keyboard())).await?;
    let kv = env.kv("STATE_STORE")?;
    // Store file_id so we know which file was referenced
    let state = UserState::AwaitingType {
        raw_text: media_type.to_string(),
        detected: None,
        media_file_id: Some(file_id.to_string()),
    };
    save_state(&kv, &format!("{}_state", chat_id), &state).await?;
    Ok(())
}

async fn handle_text(env: Env, chat_id: i64, text: String) -> Result<()> {
    let bot_token = env.secret("BOT_TOKEN")?.to_string();
    let kv = env.kv("STATE_STORE")?;
    let dedup_kv = env.kv("DEDUP_STORE")?;
    let state_key = format!("{}_state", chat_id);

    let state = load_state(&kv, &state_key).await?;

    // Issue 6: State expired (KV TTL) → notify user, don't silently reinterpret
    if state == UserState::None && !text.starts_with("http") && !text.is_empty() {
        // Check if there was a state but it expired — we can't know for sure,
        // but if user sends something that looks like a rating/comment mid-flow,
        // we should warn. Simplest: if no state and input is numeric (likely a rating),
        // tell user the draft expired.
        if text.parse::<u8>().is_ok() {
            TelegramService::send_message(
                &bot_token,
                chat_id,
                "⏰ Your previous draft expired (30 min timeout). Please start over.",
                Some(TelegramService::remove_keyboard()),
            ).await?;
            return Ok(());
        }
    }

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
            let media_file_id = match &state {
                UserState::AwaitingType { media_file_id, .. } => media_file_id.clone(),
                _ => None,
            };

            // Issue 4: Use detected.title as the item title, not the raw URL
            let title = if let Some(ref d) = detected {
                if let Some(ref t) = d.title {
                    t.clone()
                } else if ParserService::is_url(&raw_text) {
                    // URL with no guessable title → use domain-based fallback
                    format!("{} link", d.provider.label())
                } else {
                    raw_text.clone()
                }
            } else {
                raw_text.clone()
            };

            let mut item = PendingItem::new(title, kt.clone());
            item.source = "telegram".to_string();
            if let Some(d) = detected {
                item.provider = d.provider;
                item.url = Some(d.url);
                item.description = d.description;
            }
            if let Some(fid) = media_file_id {
                item.tags.push(format!("file:{}", fid));
            }

            // Enrich GitHub repos with real metadata (stars/language/topics) via API
            // instead of relying on the URL-guessed title.
            if item.provider == ResourceProvider::Github {
                if let Some(ref url) = item.url {
                    if let Some(owner_repo) = Resolver::parse_github_url(url) {
                        match Resolver::resolve_github(&env, &owner_repo).await {
                            Ok(Some(resolved)) => {
                                item.title = resolved.title;
                                item.description = resolved.description.or(item.description);
                                item.language = resolved.language;
                                item.stars = resolved.stars;
                                if !resolved.tags.is_empty() {
                                    item.tags.extend(resolved.tags);
                                }
                            }
                            Ok(None) => {
                                log_event!("warn", "resolver.github.not_found", "repo={}", owner_repo);
                            }
                            Err(e) => {
                                log_event!("error", "resolver.github.failed", "error={:?}", e);
                            }
                        }
                    }
                }
            }

            if kt.has_status_options() {
                let status_kb = TelegramService::status_keyboard(&kt);
                let state = UserState::AwaitingStatus { item };
                save_state(&kv, &state_key, &state).await?;
                TelegramService::send_message(&bot_token, chat_id, &format!("{} Status?", kt.emoji()), Some(status_kb)).await?;
            } else {
                save_and_finish(env, &bot_token, &dedup_kv, chat_id, item).await?;
            }
        }
        TextTransition::SelectStatus(status) => {
            if let UserState::AwaitingStatus { mut item } = state {
                item.status = status;
                let state = UserState::AwaitingRating { item };
                save_state(&kv, &state_key, &state).await?;
                TelegramService::send_message(&bot_token, chat_id, "Rate 1-10 or skip:", Some(TelegramService::skip_keyboard())).await?;
            }
        }
        TextTransition::SetRating(rating) => {
            if let UserState::AwaitingRating { mut item } = state {
                item.rating = Some(rating);
                let state = UserState::AwaitingComment { item };
                save_state(&kv, &state_key, &state).await?;
                TelegramService::send_message(&bot_token, chat_id, "Add a comment or skip:", Some(TelegramService::skip_keyboard())).await?;
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
        TextTransition::ConfirmAi => {
            if let UserState::AwaitingAiConfirm { item } = state {
                let kt = item.knowledge_type.clone();
                if kt.has_status_options() {
                    let status_kb = TelegramService::status_keyboard(&kt);
                    let state = UserState::AwaitingStatus { item };
                    save_state(&kv, &state_key, &state).await?;
                    TelegramService::send_message(&bot_token, chat_id, &format!("{} Status?", kt.emoji()), Some(status_kb)).await?;
                } else {
                    save_and_finish(env, &bot_token, &dedup_kv, chat_id, item).await?;
                }
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
        let state = UserState::AwaitingType {
            raw_text: text.to_string(),
            detected: Some(detected),
            media_file_id: None,
        };
        save_state(&kv, &format!("{}_state", chat_id), &state).await?;
    } else {
        // Issue 2 + 4: Use AI to analyze text-only input, then ask user to confirm
        match AiService::analyze_content(&env, text).await {
            Ok(Some(mut item)) => {
                item.source = "telegram".to_string();
                let preview = AiService::format_preview(&item);
                let state = UserState::AwaitingAiConfirm { item };
                let state_kv = env.kv("STATE_STORE")?;
                save_state(&state_kv, &format!("{}_state", chat_id), &state).await?;
                TelegramService::send_message(bot_token, chat_id, &preview, Some(TelegramService::confirm_ai_keyboard())).await?;
            }
            _ => {
                // Fallback: ask user to pick type
                TelegramService::send_message(bot_token, chat_id, "Detected title.\n\nWhat type?", Some(TelegramService::type_keyboard())).await?;
                let kv = env.kv("STATE_STORE")?;
                let state = UserState::AwaitingType {
                    raw_text: text.to_string(),
                    detected: None,
                    media_file_id: None,
                };
                save_state(&kv, &format!("{}_state", chat_id), &state).await?;
            }
        }
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
    preview.push_str(&format!("📌 Status: {}\n", item.status.label(&item.knowledge_type)));
    if let Some(r) = item.rating { preview.push_str(&format!("⭐ {}/10\n", r)); }
    if let Some(ref c) = item.comment { preview.push_str(&format!("💬 \"{}\"\n", c)); }
    preview.push_str("\nSave?");
    preview
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
        UserState::AwaitingStatus { .. } => "awaiting_status",
        UserState::AwaitingRating { .. } => "awaiting_rating",
        UserState::AwaitingComment { .. } => "awaiting_comment",
        UserState::AwaitingAiConfirm { .. } => "awaiting_ai_confirm",
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
            state_name(&UserState::AwaitingType { raw_text: "test".to_string(), detected: None, media_file_id: None }),
            "awaiting_type"
        );
    }
}