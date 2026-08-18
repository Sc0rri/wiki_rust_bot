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

/// Persists the current thread-local log buffer to KV for the given chat_id.
/// The buffer is drained and appended to the KV entry `{chat_id}_logbuf`
/// (JSON-serialized Vec<String>) with the same 30-minute TTL as dialog state.
/// This lets logs survive between requests/isolates without an immediate GitHub commit.
async fn persist_logs_to_kv(env: &Env, chat_id: i64) {
    let lines = crate::logger::take_logs();
    if lines.is_empty() {
        return;
    }
    let kv = match env.kv("STATE_STORE") {
        Ok(kv) => kv,
        Err(_) => return,
    };
    let key = format!("{}_logbuf", chat_id);
    let existing = kv
        .get(&key)
        .text()
        .await
        .unwrap_or_default()
        .unwrap_or_default();
    let mut all_lines: Vec<String> = if existing.is_empty() {
        Vec::new()
    } else {
        serde_json::from_str(&existing).unwrap_or_default()
    };
    all_lines.extend(lines);
    if let Ok(json) = serde_json::to_string(&all_lines) {
        if let Ok(put) = kv.put(&key, &json) {
            let _ = put.expiration_ttl(STATE_TTL_SECONDS).execute().await;
        }
    }
}

/// Retrieves and deletes the KV-persisted log buffer for the given chat_id,
/// then merges it with the current thread-local buffer. Returns the combined
/// lines. Used by `save_to_inbox` and `/cancel`/`/clear` to ensure no logs
/// are lost.
async fn collect_logs_for_chat(env: &Env, chat_id: i64) -> Vec<String> {
    let mut lines = crate::logger::take_logs();
    let kv = match env.kv("STATE_STORE") {
        Ok(kv) => kv,
        Err(_) => return lines,
    };
    let key = format!("{}_logbuf", chat_id);
    if let Ok(Some(existing)) = kv.get(&key).text().await {
        if !existing.is_empty() {
            if let Ok(mut kv_lines) = serde_json::from_str::<Vec<String>>(&existing) {
                lines.append(&mut kv_lines);
            }
        }
        let _ = kv.delete(&key).await;
    }
    lines
}

/// Flushes all logs (current buffer + KV-persisted) to GitHub for the given
/// chat_id, via `flush_logs_only` in a background task. Called explicitly by
/// `/cancel` and `/clear` commands.
async fn flush_logs_for_chat(env: &Env, ctx: &Context, chat_id: i64) {
    let all_lines = collect_logs_for_chat(env, chat_id).await;
    if all_lines.is_empty() {
        return;
    }
    let env_clone = env.clone();
    ctx.wait_until(async move {
        GitHubService::flush_logs_only(&env_clone, &all_lines).await;
    });
}

/// Handles an incoming Telegram webhook update.
///
/// Parses the JSON update, validates the sender's username against the allowed
/// list, and routes the update through the bot's state machine. Supports:
/// - Regular messages (text, photos, documents)
/// - Callback queries (keyboard button presses)
/// - Forwarded messages (routed to Notes)
/// - Reply messages (clarification replies with `[ref:<id>]` markers)
///
/// State transitions are persisted to Cloudflare KV with a 30-minute TTL.
/// Pending items are committed to GitHub after processing.
pub async fn handle_update(env: Env, ctx: Context, update_raw: String) -> Result<()> {
    let update: Update = match serde_json::from_str(&update_raw) {
        Ok(update) => update,
        Err(err) => {
            log_event!("warn", "telegram.update.invalid_json", "error={}", err);
            // No chat_id available — flush to GitHub immediately.
            flush_remaining_logs_if_needed(&env, &ctx).await;
            return Ok(());
        }
    };

    let allowed_username = get_env_or_secret(&env, "ALLOWED_USERNAME", "");
    if allowed_username.is_empty() {
        log_event!("error", "config.allowed_username_missing");
        // No chat_id available — flush to GitHub immediately.
        flush_remaining_logs_if_needed(&env, &ctx).await;
        return Ok(());
    }

    // Whether to write logs to inbox/logs/ in the GitHub repo.
    // Default: true. Set LOG_TO_FILE=false in env to disable.
    let log_to_file = get_env_or_secret(&env, "LOG_TO_FILE", "true") == "true";
    crate::logger::set_log_enabled(log_to_file);

    if let Some(msg) = update.message {
        let sender = msg.from.as_ref();
        if !username_is_allowed(sender.and_then(|u| u.username.as_ref()), &allowed_username) {
            persist_logs_to_kv(&env, msg.chat.id).await;
            return Ok(());
        }

        let chat_id = msg.chat.id;

        // Log incoming message details — goes into the buffer and will be
        // persisted to KV until the next save or /cancel /clear.
        {
            let has_text = msg.text.is_some();
            let has_caption = msg.caption.is_some();
            let has_photo = msg.photo.is_some();
            let has_document = msg.document.is_some();
            let has_reply = msg.reply_to_message.is_some();
            let has_forward = msg.forward_origin.is_some();
            let text_preview = msg
                .text
                .as_deref()
                .unwrap_or("")
                .chars()
                .take(50)
                .collect::<String>();
            log_event!(
                "debug",
                "telegram.message.incoming",
                "chat_id={} from={:?} text_preview=\"{}\" has_text={} has_caption={} has_photo={} has_document={} has_reply={} has_forward={}",
                chat_id,
                msg.from.as_ref().map(|u| u.id),
                text_preview,
                has_text,
                has_caption,
                has_photo,
                has_document,
                has_reply,
                has_forward,
            );
        }

        // A reply to one of our own clarifying questions ("[ref:<id>]" in
        // the text we sent) — handle it before any of the normal capture
        // flows, since it's not a new item, it's an answer to an old one.
        if let Some(replied) = msg.reply_to_message.as_ref() {
            if let Some(replied_text) = replied.text.as_ref() {
                if let Some(start) = replied_text.find("[ref:") {
                    if let Some(end) = replied_text[start..].find(']') {
                        let item_id = &replied_text[start + 5..start + end];
                        let answer = msg.text.clone().unwrap_or_default();
                        let item_id = item_id.to_string();
                        let env_clone = env.clone();
                        match GitHubService::save_reply_to_inbox(&env_clone, &item_id, &answer)
                            .await
                        {
                            Ok(_) => {
                                let bot_token = get_env_or_secret(&env_clone, "BOT_TOKEN", "");
                                let _ = TelegramService::send_message(
                                    &bot_token,
                                    chat_id,
                                    "Спасибо, уточнил(а)! 🙌",
                                    None,
                                )
                                .await;
                            }
                            Err(e) => {
                                log_event!("error", "clarification.reply.failed", "error={:?}", e)
                            }
                        }
                        persist_logs_to_kv(&env, chat_id).await;
                        return Ok(());
                    }
                }
            }
        }

        if let Some(photos) = &msg.photo {
            if let Some(photo) = photos.last().cloned() {
                log_event!(
                    "info",
                    "telegram.media.received",
                    "chat_id={} type=image is_forwarded={} caption={}",
                    chat_id,
                    msg.forward_origin.is_some(),
                    msg.caption
                        .as_deref()
                        .unwrap_or_default()
                        .chars()
                        .take(80)
                        .collect::<String>()
                );
                let file_id = photo.file_id.clone();
                let caption = msg.caption.clone();
                let is_forwarded = msg.forward_origin.is_some();
                let meta = MediaMeta {
                    width: Some(photo.width),
                    height: Some(photo.height),
                    mime: Some("image/jpeg".to_string()), // Telegram always sends photos as JPEG
                };
                let env_clone = env.clone();
                if let Err(e) = handle_media(
                    env_clone,
                    chat_id,
                    "image",
                    &file_id,
                    caption,
                    is_forwarded,
                    meta,
                )
                .await
                {
                    log_event!("error", "telegram.photo.failed", "error={:?}", e);
                }
                persist_logs_to_kv(&env, chat_id).await;
                return Ok(());
            }
        }

        if let Some(doc) = msg.document.as_ref().cloned() {
            log_event!(
                "info",
                "telegram.document.received",
                "chat_id={} filename={} mime={}",
                chat_id,
                doc.file_name.as_deref().unwrap_or("<unknown>"),
                doc.mime_type.as_deref().unwrap_or("<unknown>")
            );
            let caption = msg.caption.clone();
            let is_forwarded = msg.forward_origin.is_some();
            let meta = MediaMeta {
                width: None,
                height: None,
                mime: doc.mime_type.clone(),
            };
            let env_clone = env.clone();
            let file_name = doc.file_name.unwrap_or_default();
            let file_id = doc.file_id.clone();
            if file_name.to_lowercase().ends_with(".pdf") {
                if let Err(e) = handle_media(
                    env_clone,
                    chat_id,
                    "pdf",
                    &file_id,
                    caption,
                    is_forwarded,
                    meta,
                )
                .await
                {
                    log_event!("error", "telegram.pdf.failed", "error={:?}", e);
                }
            }
            persist_logs_to_kv(&env, chat_id).await;
        }

        let text = msg.text.clone().unwrap_or_default().trim().to_string();
        if text.is_empty() {
            persist_logs_to_kv(&env, chat_id).await;
            return Ok(());
        }

        if msg.forward_origin.is_some() {
            log_event!(
                "info",
                "telegram.forwarded.received",
                "chat_id={} text_chars={}",
                chat_id,
                text.chars().count()
            );
            let env_clone = env.clone();
            let text_clone = text.clone();
            if let Err(e) = handle_forwarded(env_clone, chat_id, text_clone).await {
                log_event!("error", "telegram.forwarded.failed", "error={:?}", e);
            }
            persist_logs_to_kv(&env, chat_id).await;
            return Ok(());
        }

        if text.starts_with('/') {
            let env_clone = env.clone();
            if let Err(e) = handle_command(env_clone, &ctx, chat_id, &text).await {
                log_event!("error", "telegram.command.failed", "error={:?}", e);
            }
            // Command path: /cancel and /clear flush logs to GitHub inside handle_command.
            // All other commands persist to KV.
            persist_logs_to_kv(&env, chat_id).await;
            return Ok(());
        }

        log_event!(
            "info",
            "telegram.text.received",
            "chat_id={} text={}",
            chat_id,
            text.chars().count()
        );
        let env_clone = env.clone();
        if let Err(e) = handle_text(env_clone, chat_id, text).await {
            log_event!("error", "telegram.text.failed", "error={:?}", e);
        }

        // Persist remaining logs (from text processing) to KV so they survive
        // between requests. They'll be committed on next save or /cancel /clear.
        persist_logs_to_kv(&env, chat_id).await;
    }

    Ok(())
}

/// Flushes any buffered log lines to inbox/logs/<date>.log via a background
/// task. Only used for early returns where chat_id is unknown (invalid JSON,
/// missing config). All other paths use `persist_logs_to_kv` instead.
async fn flush_remaining_logs_if_needed(env: &Env, ctx: &Context) {
    let env_clone = env.clone();
    let remaining = crate::logger::take_logs();
    if !remaining.is_empty() {
        ctx.wait_until(async move {
            GitHubService::flush_logs_only(&env_clone, &remaining).await;
        });
    }
}

fn username_is_allowed(username: Option<&String>, allowed: &str) -> bool {
    username.map(|u| u.as_str()).unwrap_or_default() == allowed
}

async fn handle_command(env: Env, ctx: &Context, chat_id: i64, text: &str) -> Result<()> {
    let bot_token = env.secret("BOT_TOKEN")?.to_string();
    let command = text.split_whitespace().next().unwrap_or("").to_lowercase();

    let reply: String = match command.as_str() {
        "/start" => {
            "👋 Send a link, a photo, a PDF, or just type something to add it to your wiki inbox."
                .to_string()
        }
        "/cancel" => {
            let kv = env.kv("STATE_STORE")?;
            delete_state(&kv, &format!("{}_state", chat_id), chat_id).await?;
            // Flush accumulated logs to GitHub before clearing state.
            flush_logs_for_chat(&env, ctx, chat_id).await;
            "❌ Cancelled.".to_string()
        }
        "/clear" => {
            let dedup_kv = env.kv("DEDUP_STORE")?;
            match DedupService::clear_all(&dedup_kv).await {
                Ok(count) => {
                    // Flush accumulated logs to GitHub before clearing state.
                    flush_logs_for_chat(&env, ctx, chat_id).await;
                    format!(
                        "🧹 Cleared {} dedup entries. Everything will be treated as new again.",
                        count
                    )
                }
                Err(e) => {
                    log_event!("error", "dedup.clear.failed", "error={:?}", e);
                    format!("❌ Couldn't clear dedup store: {}", e)
                }
            }
        }
        _ => "Unknown command. Try /start.".to_string(),
    };

    TelegramService::send_message(
        &bot_token,
        chat_id,
        &reply,
        Some(TelegramService::remove_keyboard()),
    )
    .await?;
    Ok(())
}

async fn handle_forwarded(env: Env, chat_id: i64, text: String) -> Result<()> {
    let bot_token = env.secret("BOT_TOKEN")?.to_string();
    let dedup_kv = env.kv("DEDUP_STORE")?;

    let mut item = PendingItem::new(text, KnowledgeType::Note, chat_id);
    item.source = "telegram".to_string();
    item.raw_text = Some(item.title.clone());
    item.tags.push("forwarded".to_string());

    save_and_finish(env, &bot_token, &dedup_kv, chat_id, item).await?;
    Ok(())
}

struct MediaMeta {
    width: Option<i64>,
    height: Option<i64>,
    mime: Option<String>,
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    digest.iter().map(|b| format!("{:02x}", b)).collect()
}

async fn handle_media(
    env: Env,
    chat_id: i64,
    media_type: &str,
    file_id: &str,
    caption: Option<String>,
    is_forwarded: bool,
    meta: MediaMeta,
) -> Result<()> {
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

    let mut item = PendingItem::new(title, KnowledgeType::Note, chat_id);
    item.source = "telegram".to_string();
    item.asset_width = meta.width;
    item.asset_height = meta.height;
    item.asset_mime = meta.mime;
    if let Some(ref c) = caption {
        let trimmed = c.trim();
        if !trimmed.is_empty() {
            item.raw_text = Some(trimmed.to_string());
        }
    }
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
                item.asset_sha256 = Some(sha256_hex(&bytes));
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
    TelegramService::send_message(
        &bot_token,
        chat_id,
        &format!("{}\nAdd a comment or skip:", status_line),
        Some(TelegramService::skip_keyboard()),
    )
    .await?;
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
            )
            .await?;
            return Ok(());
        }
    }

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
        TextTransition::SelectType(kt) => match state {
            UserState::AwaitingType { raw_text, .. } => {
                let mut item = PendingItem::new(raw_text, kt.clone(), chat_id);
                item.source = "telegram".to_string();
                item.raw_text = Some(item.title.clone());
                proceed_with_item(
                    env, &bot_token, &kv, &dedup_kv, &state_key, chat_id, kt, item,
                )
                .await?;
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
                TelegramService::send_message(
                    &bot_token,
                    chat_id,
                    "Add a comment or skip:",
                    Some(TelegramService::skip_keyboard()),
                )
                .await?;
            }
        }
        TextTransition::SetComment(comment) => {
            if let UserState::AwaitingComment { mut item } = state {
                item.comment = if comment.is_empty() {
                    None
                } else {
                    Some(comment)
                };
                delete_state(&kv, &state_key, chat_id).await?;
                save_and_finish(env, &bot_token, &dedup_kv, chat_id, item).await?;
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
    if matches!(
        item.knowledge_type,
        KnowledgeType::Series | KnowledgeType::Anime
    ) {
        let state = UserState::AwaitingSeason { item };
        save_state(kv, state_key, &state).await?;
        TelegramService::send_message(
            bot_token,
            chat_id,
            "Season? (number or skip)",
            Some(TelegramService::skip_keyboard()),
        )
        .await?;
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
        TelegramService::send_message(
            bot_token,
            chat_id,
            "Rate 1-10 or skip:",
            Some(TelegramService::skip_keyboard()),
        )
        .await?;
    } else {
        let state = UserState::AwaitingComment { item };
        save_state(kv, state_key, &state).await?;
        TelegramService::send_message(
            bot_token,
            chat_id,
            "Add a comment or skip:",
            Some(TelegramService::skip_keyboard()),
        )
        .await?;
    }
    Ok(())
}

/// Shared continuation after a type is known (from manual pick).
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
        TelegramService::send_message(
            bot_token,
            chat_id,
            &format!("{} Status?", kt.emoji()),
            Some(status_kb),
        )
        .await?;
    } else {
        delete_state(kv, state_key, chat_id).await?;
        save_and_finish(env, bot_token, dedup_kv, chat_id, item).await?;
    }
    Ok(())
}

async fn process_fresh(
    env: Env,
    bot_token: &str,
    _dedup_kv: &worker::kv::KvStore,
    chat_id: i64,
    text: &str,
) -> Result<()> {
    if ParserService::is_url(text) {
        let detected = Detector::detect(text);

        let mut item = PendingItem::new(
            detected
                .title
                .clone()
                .unwrap_or_else(|| format!("{} link", detected.provider.label())),
            KnowledgeType::Link,
            chat_id,
        );
        item.source = "telegram".to_string();
        item.raw_text = Some(text.to_string());
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
        } else if item.provider == ResourceProvider::Youtube {
            if let Some(ref url) = item.url {
                match Resolver::resolve_youtube(url).await {
                    Ok(Some((title, author))) => {
                        item.title = title;
                        item.author = author;
                    }
                    Ok(None) => {
                        log_event!("warn", "resolver.youtube.no_result", "url={}", url);
                    }
                    Err(e) => {
                        log_event!("error", "resolver.youtube.failed", "error={:?}", e);
                    }
                }
            }
        } else {
            // All remaining providers (generic web pages, forum threads,
            // Habr/arXiv/Wikipedia, etc.): always pull the real <title> and
            // meta description from the page. The URL-derived guess_title is
            // only a fallback if the fetch fails or returns nothing.
            if let Some(ref url) = item.url {
                match Resolver::resolve_web_title(url).await {
                    Ok(Some((title, description))) => {
                        item.title = title;
                        if item.description.is_none() {
                            item.description = description;
                        }
                    }
                    Ok(None) => {
                        log_event!("warn", "resolver.web.no_result", "url={}", url);
                    }
                    Err(e) => {
                        log_event!("error", "resolver.web.failed", "error={:?}", e);
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
        )
        .await?;
    } else {
        // Plain text: ask the user to pick a type manually.
        TelegramService::send_message(
            bot_token,
            chat_id,
            "What type?",
            Some(TelegramService::type_keyboard()),
        )
        .await?;
        let kv = env.kv("STATE_STORE")?;
        let state = UserState::AwaitingType {
            raw_text: text.to_string(),
            detected: None,
            media_file_id: None,
        };
        save_state(&kv, &format!("{}_state", chat_id), &state).await?;
    }
    Ok(())
}

async fn save_and_finish(
    env: Env,
    bot_token: &str,
    dedup_kv: &worker::kv::KvStore,
    chat_id: i64,
    item: PendingItem,
) -> Result<()> {
    let dedup_key = match item.asset_sha256.as_deref() {
        // Media items with an archived file: dedupe by file content, not by
        // title. Caption-less files all share the generic "PDF note"/"Image
        // note" title, so a title key would wrongly reject a second, different
        // file as "Already saved". Two identical files still share a SHA-256,
        // so resending the same document is still caught.
        Some(sha) => DedupService::hash_key(sha),
        None => DedupService::title_key(&item.title),
    };
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

    TelegramService::send_message(
        bot_token,
        chat_id,
        "⏳ Saving...",
        Some(TelegramService::remove_keyboard()),
    )
    .await?;

    // Collect both current buffer and any KV-persisted logs from previous requests.
    let log_lines = collect_logs_for_chat(&env, chat_id).await;
    match GitHubService::save_to_inbox(&env, &item, &log_lines).await {
        Ok(path) => {
            // Dedup marks are bookkeeping only — if writing them fails, the
            // save itself already succeeded and the user must still see that.
            if let Err(e) = DedupService::mark_processed(dedup_kv, &dedup_key).await {
                log_event!("warn", "dedup.mark.title_failed", "error={:?}", e);
            }
            if let Some(ref url) = item.url {
                if let Err(e) =
                    DedupService::mark_processed(dedup_kv, &DedupService::url_key(url)).await
                {
                    log_event!("warn", "dedup.mark.url_failed", "error={:?}", e);
                }
            }
            TelegramService::send_message(
                bot_token,
                chat_id,
                &format!("✅ Saved:\n{}", path),
                Some(TelegramService::remove_keyboard()),
            )
            .await?;
        }
        Err(e) => {
            crate::logger::restore_logs(&log_lines);
            // Write the error directly via append_log (Contents API) so it's
            // visible in the log file even if the Git Data API commit failed.
            let error_msg = format!("save_to_inbox failed: {:?}", e);
            GitHubService::append_log(&env, "error", "save_and_finish.failed", &error_msg).await;
            TelegramService::send_message(
                bot_token,
                chat_id,
                &format!("❌ Error: {}", e),
                Some(TelegramService::remove_keyboard()),
            )
            .await?;
        }
    }
    Ok(())
}

/// Builds a human-readable preview string for a pending item.
/// Shows emoji, title, URL, provider label, and available metadata
/// (stars, status, season, rating, comment, tags) in a Telegram-friendly format.
fn build_preview(item: &PendingItem) -> String {
    let mut preview = format!("{} {}\n", item.knowledge_type.emoji(), item.title);
    if let Some(ref url) = item.url {
        preview.push_str(&format!("🔗 {}\n", url));
    }
    if !item.provider.label().is_empty() {
        preview.push_str(&format!("📦 {}\n", item.provider.label()));
    }
    if item.stars.is_some() || item.language.is_some() {
        let mut meta = Vec::new();
        if let Some(stars) = item.stars {
            meta.push(format!("⭐ {}", stars));
        }
        if let Some(ref lang) = item.language {
            meta.push(lang.clone());
        }
        preview.push_str(&format!("{}\n", meta.join(" · ")));
    }
    if let Some(ref desc) = item.description {
        preview.push_str(&format!("📝 {}\n", desc));
    }
    if !item.tags.is_empty() {
        preview.push_str(&format!("🏷 {}\n", item.tags.join(", ")));
    }
    if item.knowledge_type.has_status_options() {
        preview.push_str(&format!(
            "📌 Status: {}\n",
            item.status.label(&item.knowledge_type)
        ));
    }
    if let Some(season) = item.season {
        preview.push_str(&format!("📀 Season {}\n", season));
    }
    if let Some(r) = item.rating {
        preview.push_str(&format!("🌟 {}/10\n", r));
    }
    if let Some(ref c) = item.comment {
        preview.push_str(&format!("💬 \"{}\"\n", c));
    }
    preview
}

/// Loads the user state from KV for a given state key.
/// Returns `UserState::None` if no state is stored.
async fn load_state(kv: &worker::kv::KvStore, state_key: &str) -> Result<UserState> {
    let Some(s) = kv.get(state_key).text().await? else {
        return Ok(UserState::None);
    };
    Ok(UserState::parse_or_none(&s))
}

/// Saves the user state to KV with a 30-minute TTL.
/// The state is JSON-serialized and stored under `state_key`.
async fn save_state(kv: &worker::kv::KvStore, state_key: &str, state: &UserState) -> Result<()> {
    kv.put(state_key, &serde_json::to_string(state)?)?
        .expiration_ttl(STATE_TTL_SECONDS)
        .execute()
        .await?;
    Ok(())
}

/// Deletes the user state from KV and logs the deletion.
/// Called after a successful item submission to clear the draft state.
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_name_should_return_correct_names() {
        assert_eq!(state_name(&UserState::None), "none");
        assert_eq!(
            state_name(&UserState::AwaitingType {
                raw_text: "test".to_string(),
                detected: None,
                media_file_id: None
            }),
            "awaiting_type"
        );
    }
}