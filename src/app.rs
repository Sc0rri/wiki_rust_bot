use crate::ai::AiService;
use crate::dedup::DedupService;
use crate::detector::Detector;
use crate::github::GitHubService;
use crate::parser::ParserService;
use crate::resolver::Resolver;
use crate::state::{KnowledgeType, PendingItem, ResourceProvider, TextTransition, UserState};
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
                let caption = msg.caption.clone();
                let is_forwarded = msg.forward_origin.is_some();
                let env_clone = env.clone();
                ctx.wait_until(async move {
                    if let Err(e) = handle_media(env_clone, chat_id, "image", &file_id, caption, is_forwarded).await {
                        log_event!("error", "telegram.photo.failed", "error={:?}", e);
                    }
                });
                return Ok(());
            }
        }

        if let Some(doc) = msg.document.as_ref().cloned() {
            let caption = msg.caption.clone();
            let is_forwarded = msg.forward_origin.is_some();
            let env_clone = env.clone();
            ctx.wait_until(async move {
                let file_name = doc.file_name.unwrap_or_default();
                let file_id = doc.file_id.clone();
                if file_name.to_lowercase().ends_with(".pdf") {
                    if let Err(e) = handle_media(env_clone, chat_id, "pdf", &file_id, caption, is_forwarded).await {
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

        if msg.forward_origin.is_some() {
            let env_clone = env.clone();
            let text_clone = text.clone();
            ctx.wait_until(async move {
                if let Err(e) = handle_forwarded(env_clone, chat_id, text_clone).await {
                    log_event!("error", "telegram.forwarded.failed", "error={:?}", e);
                }
            });
            return Ok(());
        }

        if text.starts_with('/') {
            let env_clone = env.clone();
            ctx.wait_until(async move {
                if let Err(e) = handle_command(env_clone, chat_id, &text).await {
                    log_event!("error", "telegram.command.failed", "error={:?}", e);
                }
            });
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

async fn handle_command(env: Env, chat_id: i64, text: &str) -> Result<()> {
    let bot_token = env.secret("BOT_TOKEN")?.to_string();
    let command = text.split_whitespace().next().unwrap_or("").to_lowercase();

    let reply: String = match command.as_str() {
        "/start" => "👋 Send a link, a photo, a PDF, or just type something to add it to your wiki inbox.".to_string(),
        "/cancel" => {
            let kv = env.kv("STATE_STORE")?;
            delete_state(&kv, &format!("{}_state", chat_id), chat_id).await?;
            "❌ Cancelled.".to_string()
        }
        "/clear" => {
            let dedup_kv = env.kv("DEDUP_STORE")?;
            match DedupService::clear_all(&dedup_kv).await {
                Ok(count) => format!("🧹 Cleared {} dedup entries. Everything will be treated as new again.", count),
                Err(e) => {
                    log_event!("error", "dedup.clear.failed", "error={:?}", e);
                    format!("❌ Couldn't clear dedup store: {}", e)
                }
            }
        }
        _ => "Unknown command. Try /start.".to_string(),
    };

    TelegramService::send_message(&bot_token, chat_id, &reply, Some(TelegramService::remove_keyboard())).await?;
    Ok(())
}

async fn handle_forwarded(env: Env, chat_id: i64, text: String) -> Result<()> {
    let bot_token = env.secret("BOT_TOKEN")?.to_string();
    let dedup_kv = env.kv("DEDUP_STORE")?;

    let mut item = PendingItem::new(text, KnowledgeType::Note);
    item.source = "telegram".to_string();
    item.tags.push("forwarded".to_string());

    save_and_finish(env, &bot_token, &dedup_kv, chat_id, item).await?;
    Ok(())
}

async fn handle_media(env: Env, chat_id: i64, media_type: &str, file_id: &str, caption: Option<String>, is_forwarded: bool) -> Result<()> {
    let bot_token = env.secret("BOT_TOKEN")?.to_string();

    let label = match media_type {
        "image" => "Image",
        "pdf" => "PDF",
        _ => return Ok(()),
    };
    let extension = if media_type == "pdf" { "pdf" } else { "jpg" };

    let title = caption
        .as_deref()
        .map(|c| c.trim())
        .filter(|c| !c.is_empty())
        .map(|c| c.to_string())
        .unwrap_or_else(|| format!("{} note", label));

    let mut item = PendingItem::new(title, KnowledgeType::Note);
    item.source = "telegram".to_string();
    if is_forwarded {
        item.tags.push("forwarded".to_string());
    }

    // Best-effort archive: download the file from Telegram and commit it to
    // inbox/assets/, since file_id isn't a durable reference (it can expire
    // and only ever resolves within this bot's own token). If any step fails,
    // fall back to tagging the file_id so the item still gets captured — but
    // tell the user in chat, not just in logs, so a failed archive isn't
    // mistaken for a successful one.
    let archived = match TelegramService::get_file_path(&bot_token, file_id).await {
        Ok(Some(file_path)) => match TelegramService::download_file(&bot_token, &file_path).await {
            Ok(bytes) => {
                let asset_filename = ParserService::generate_asset_filename(&item, extension);
                match GitHubService::save_asset(&env, &asset_filename, &bytes).await {
                    Ok(asset_path) => {
                        item.tags.push(format!("asset:{}", asset_path));
                        true
                    }
                    Err(e) => {
                        log_event!("error", "github.asset.save_failed", "error={:?}", e);
                        item.tags.push(format!("file:{}", file_id));
                        false
                    }
                }
            }
            Err(e) => {
                log_event!("error", "telegram.file.download_failed", "error={:?}", e);
                item.tags.push(format!("file:{}", file_id));
                false
            }
        },
        Ok(None) => {
            log_event!("warn", "telegram.getfile.no_path", "file_id={}", file_id);
            item.tags.push(format!("file:{}", file_id));
            false
        }
        Err(e) => {
            log_event!("error", "telegram.getfile.failed", "error={:?}", e);
            item.tags.push(format!("file:{}", file_id));
            false
        }
    };

    // Forwarded media may arrive as several separate Telegram updates in a
    // row (an album forwarded together) — each would otherwise overwrite the
    // same chat's pending-comment state in KV and silently drop earlier
    // items. So forwarded media saves immediately, same as forwarded text,
    // instead of waiting for a per-item comment reply.
    if is_forwarded {
        let dedup_kv = env.kv("DEDUP_STORE")?;
        save_and_finish(env, &bot_token, &dedup_kv, chat_id, item).await?;
        return Ok(());
    }

    let kv = env.kv("STATE_STORE")?;
    let state = UserState::AwaitingComment { item };
    save_state(&kv, &format!("{}_state", chat_id), &state).await?;
    let status_line = if archived {
        "📎 File archived to inbox/assets/."
    } else {
        "⚠️ Couldn't archive the file (network/GitHub error) — saved a reference only, check logs."
    };
    TelegramService::send_message(&bot_token, chat_id, &format!("{}\nAdd a comment or skip:", status_line), Some(TelegramService::skip_keyboard())).await?;
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
        TextTransition::SelectType(kt) => match state {
            UserState::AwaitingType { raw_text, .. } => {
                let mut item = PendingItem::new(raw_text, kt.clone());
                item.source = "telegram".to_string();
                proceed_with_item(env, &bot_token, &kv, &dedup_kv, &state_key, chat_id, kt, item).await?;
            }
            UserState::AwaitingAiConfirm { mut item } => {
                item.knowledge_type = kt.clone();
                proceed_with_item(env, &bot_token, &kv, &dedup_kv, &state_key, chat_id, kt, item).await?;
            }
            _ => {}
        },
        TextTransition::SelectStatus(status) => {
            if let UserState::AwaitingStatus { mut item } = state {
                item.status = status;
                proceed_after_status(&kv, &bot_token, &state_key, chat_id, item).await?;
            }
        }
        TextTransition::SetSeason(season) => {
            if let UserState::AwaitingSeason { mut item } = state {
                item.season = season;
                proceed_after_season(&kv, &bot_token, &state_key, chat_id, item).await?;
            }
        }
        TextTransition::SetRating(rating) => {
            if let UserState::AwaitingRating { mut item } = state {
                item.rating = if rating == 0 { None } else { Some(rating) };
                let state = UserState::AwaitingComment { item };
                save_state(&kv, &state_key, &state).await?;
                TelegramService::send_message(&bot_token, chat_id, "Add a comment or skip:", Some(TelegramService::skip_keyboard())).await?;
            }
        }
        TextTransition::SetComment(comment) => {
            if let UserState::AwaitingComment { mut item } = state {
                item.comment = if comment.is_empty() { None } else { Some(comment) };
                delete_state(&kv, &state_key, chat_id).await?;
                save_and_finish(env, &bot_token, &dedup_kv, chat_id, item).await?;
            }
        }
        TextTransition::ConfirmAi => {
            if let UserState::AwaitingAiConfirm { item } = state {
                let kt = item.knowledge_type.clone();
                proceed_with_item(env, &bot_token, &kv, &dedup_kv, &state_key, chat_id, kt, item).await?;
            }
        }
        TextTransition::ProcessFresh => {
            delete_state(&kv, &state_key, chat_id).await?;
            process_fresh(env, &bot_token, &dedup_kv, chat_id, &text).await?;
        }
    }

    Ok(())
}

/// After status is set: Series/Anime get an extra "what season" prompt before
/// rating/comment; everything else skips straight to proceed_after_season.
async fn proceed_after_status(
    kv: &worker::kv::KvStore,
    bot_token: &str,
    state_key: &str,
    chat_id: i64,
    item: PendingItem,
) -> Result<()> {
    if matches!(item.knowledge_type, KnowledgeType::Series | KnowledgeType::Anime) {
        let state = UserState::AwaitingSeason { item };
        save_state(kv, state_key, &state).await?;
        TelegramService::send_message(bot_token, chat_id, "Season? (number or skip)", Some(TelegramService::skip_keyboard())).await?;
    } else {
        proceed_after_season(kv, bot_token, state_key, chat_id, item).await?;
    }
    Ok(())
}

async fn proceed_after_season(
    kv: &worker::kv::KvStore,
    bot_token: &str,
    state_key: &str,
    chat_id: i64,
    item: PendingItem,
) -> Result<()> {
    if item.status.needs_rating() {
        let state = UserState::AwaitingRating { item };
        save_state(kv, state_key, &state).await?;
        TelegramService::send_message(bot_token, chat_id, "Rate 1-10 or skip:", Some(TelegramService::skip_keyboard())).await?;
    } else {
        let state = UserState::AwaitingComment { item };
        save_state(kv, state_key, &state).await?;
        TelegramService::send_message(bot_token, chat_id, "Add a comment or skip:", Some(TelegramService::skip_keyboard())).await?;
    }
    Ok(())
}

/// Shared continuation after a type is known (from manual pick or AI confirm).
/// This path only ever produces Book/Movie/Series/Anime/Note (Link is built
/// and handled separately in process_fresh) — and a text-only Note has
/// nothing worth commenting on, so it saves immediately. Media types go on
/// to status/rating/comment as usual.
async fn proceed_with_item(
    env: Env,
    bot_token: &str,
    kv: &worker::kv::KvStore,
    dedup_kv: &worker::kv::KvStore,
    state_key: &str,
    chat_id: i64,
    kt: KnowledgeType,
    item: PendingItem,
) -> Result<()> {
    if kt.has_status_options() {
        let status_kb = TelegramService::status_keyboard(&kt);
        let state = UserState::AwaitingStatus { item };
        save_state(kv, state_key, &state).await?;
        TelegramService::send_message(bot_token, chat_id, &format!("{} Status?", kt.emoji()), Some(status_kb)).await?;
    } else {
        delete_state(kv, state_key, chat_id).await?;
        save_and_finish(env, bot_token, dedup_kv, chat_id, item).await?;
    }
    Ok(())
}

async fn process_fresh(env: Env, bot_token: &str, _dedup_kv: &worker::kv::KvStore, chat_id: i64, text: &str) -> Result<()> {
    if ParserService::is_url(text) {
        let detected = Detector::detect(text);

        let mut item = PendingItem::new(
            detected.title.clone().unwrap_or_else(|| format!("{} link", detected.provider.label())),
            KnowledgeType::Link,
        );
        item.source = "telegram".to_string();
        item.provider = detected.provider.clone();
        item.url = Some(detected.url.clone());
        item.description = detected.description.clone();

        // Enrich GitHub repos with real metadata (stars/language/topics) via API.
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

        // Links skip type/status entirely — show what was found (including
        // GitHub stars/language, so the enrichment is actually visible in
        // chat and not just in the committed file) and ask for a comment.
        let preview = build_preview(&item);
        let kv = env.kv("STATE_STORE")?;
        let state = UserState::AwaitingComment { item };
        save_state(&kv, &format!("{}_state", chat_id), &state).await?;
        TelegramService::send_message(
            bot_token,
            chat_id,
            &format!("{}\nAdd a comment or skip:", preview),
            Some(TelegramService::skip_keyboard()),
        ).await?;
    } else {
        // Plain text: let AI decide if it's a Book/Movie/Series/Anime — anything
        // else (or an AI failure) falls back to a manual pick from those four + Note.
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
                TelegramService::send_message(bot_token, chat_id, "Couldn't detect type automatically.\n\nWhat type?", Some(TelegramService::type_keyboard())).await?;
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
            // Dedup marks are bookkeeping only — if writing them fails, the
            // save itself already succeeded and the user must still see that.
            if let Err(e) = DedupService::mark_processed(dedup_kv, &dedup_key).await {
                log_event!("warn", "dedup.mark.title_failed", "error={:?}", e);
            }
            if let Some(ref url) = item.url {
                if let Err(e) = DedupService::mark_processed(dedup_kv, &DedupService::url_key(url)).await {
                    log_event!("warn", "dedup.mark.url_failed", "error={:?}", e);
                }
            }
            TelegramService::send_message(bot_token, chat_id, &format!("✅ Saved:\n{}", path), Some(TelegramService::remove_keyboard())).await?;
        }
        Err(e) => TelegramService::send_message(bot_token, chat_id, &format!("❌ Error: {}", e), Some(TelegramService::remove_keyboard())).await?,
    }
    Ok(())
}

fn build_preview(item: &PendingItem) -> String {
    let mut preview = format!("{} {}\n", item.knowledge_type.emoji(), item.title);
    if let Some(ref url) = item.url { preview.push_str(&format!("🔗 {}\n", url)); }
    if !item.provider.label().is_empty() { preview.push_str(&format!("📦 {}\n", item.provider.label())); }
    if item.stars.is_some() || item.language.is_some() {
        let mut meta = Vec::new();
        if let Some(stars) = item.stars { meta.push(format!("⭐ {}", stars)); }
        if let Some(ref lang) = item.language { meta.push(lang.clone()); }
        preview.push_str(&format!("{}\n", meta.join(" · ")));
    }
    if item.knowledge_type.has_status_options() {
        preview.push_str(&format!("📌 Status: {}\n", item.status.label(&item.knowledge_type)));
    }
    if let Some(season) = item.season { preview.push_str(&format!("📀 Season {}\n", season)); }
    if let Some(r) = item.rating { preview.push_str(&format!("🌟 {}/10\n", r)); }
    if let Some(ref c) = item.comment { preview.push_str(&format!("💬 \"{}\"\n", c)); }
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

#[cfg(test)]
fn state_name(state: &UserState) -> &'static str {
    match state {
        UserState::None => "none",
        UserState::AwaitingType { .. } => "awaiting_type",
        UserState::AwaitingStatus { .. } => "awaiting_status",
        UserState::AwaitingSeason { .. } => "awaiting_season",
        UserState::AwaitingRating { .. } => "awaiting_rating",
        UserState::AwaitingComment { .. } => "awaiting_comment",
        UserState::AwaitingAiConfirm { .. } => "awaiting_ai_confirm",
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