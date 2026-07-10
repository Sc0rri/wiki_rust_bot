use serde::{Deserialize, Serialize};
use worker::*;

use crate::state::KnowledgeType;

// Type buttons — only the four media types get a full status/rating/comment
// flow; everything else is a Link (URL) or a Note (freeform text/media).
pub const BTN_BOOK: &str = "📚 Book";
pub const BTN_MOVIE: &str = "🎬 Movie";
pub const BTN_SERIES: &str = "📺 Series";
pub const BTN_ANIME: &str = "🎌 Anime";
pub const BTN_NOTE: &str = "📝 Note";

// Status buttons
pub const BTN_BACKLOG: &str = "📋 Backlog";
pub const BTN_DONE: &str = "✅ Done";
pub const BTN_DROPPED: &str = "❌ Dropped";

// Common buttons
pub const BTN_CANCEL: &str = "❌ Cancel";
pub const BTN_SKIP: &str = "⏭ Skip";

#[derive(Deserialize, Serialize, Debug)]
pub struct Update {
    pub message: Option<Message>,
    pub callback_query: Option<CallbackQuery>,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct User {
    pub id: i64,
    pub username: Option<String>,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct Chat {
    pub id: i64,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct PhotoSize {
    pub file_id: String,
    pub file_unique_id: String,
    pub width: i64,
    pub height: i64,
    pub file_size: Option<i64>,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct Message {
    pub text: Option<String>,
    pub chat: Chat,
    pub from: Option<User>,
    pub photo: Option<Vec<PhotoSize>>,
    pub document: Option<Document>,
    /// Present on forwarded messages. We don't need to parse its contents —
    /// just knowing a message was forwarded is enough to route it to Note.
    pub forward_origin: Option<serde_json::Value>,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct Document {
    pub file_id: String,
    pub file_name: Option<String>,
    pub mime_type: Option<String>,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct CallbackQuery {
    pub id: String,
    pub from: User,
    pub message: Option<Message>,
    pub data: Option<String>,
}

pub struct TelegramService;

impl TelegramService {
    pub async fn send_message(
        bot_token: &str,
        chat_id: i64,
        text: &str,
        keyboard: Option<serde_json::Value>,
    ) -> Result<()> {
        let url = format!("https://api.telegram.org/bot{}/sendMessage", bot_token);
        let mut payload = serde_json::json!({
            "chat_id": chat_id,
            "text": text,
        });
        if let Some(kb) = keyboard {
            payload
                .as_object_mut()
                .unwrap()
                .insert("reply_markup".to_string(), kb);
        }
        send_telegram_api(&url, &payload).await
    }

    /// Resolves a file_id to its current file_path via Telegram's getFile API.
    /// file_id is stable but file_path/the download URL is short-lived — this
    /// must be called right before downloading, not cached.
    pub async fn get_file_path(bot_token: &str, file_id: &str) -> Result<Option<String>> {
        let url = format!("https://api.telegram.org/bot{}/getFile?file_id={}", bot_token, file_id);
        let mut req_init = RequestInit::new();
        req_init.with_method(Method::Get);
        let req = Request::new_with_init(&url, &req_init)?;
        let mut resp = Fetch::Request(req).send().await?;
        if resp.status_code() != 200 {
            let body = resp.text().await?;
            crate::log_event!("warn", "telegram.getfile.failed", "status={} body_chars={}", resp.status_code(), body.chars().count());
            return Ok(None);
        }
        let value: serde_json::Value = resp.json().await?;
        Ok(value
            .get("result")
            .and_then(|r| r.get("file_path"))
            .and_then(|p| p.as_str())
            .map(|s| s.to_string()))
    }

    /// Downloads the raw bytes of a file previously resolved via get_file_path.
    pub async fn download_file(bot_token: &str, file_path: &str) -> Result<Vec<u8>> {
        let url = format!("https://api.telegram.org/file/bot{}/{}", bot_token, file_path);
        let mut req_init = RequestInit::new();
        req_init.with_method(Method::Get);
        let req = Request::new_with_init(&url, &req_init)?;
        let mut resp = Fetch::Request(req).send().await?;
        if resp.status_code() != 200 {
            return Err(worker::Error::from(format!("Telegram file download failed: status {}", resp.status_code())));
        }
        resp.bytes().await
    }

    pub fn type_keyboard() -> serde_json::Value {
        serde_json::json!({
            "keyboard": [
                [
                    {"text": BTN_BOOK},
                    {"text": BTN_MOVIE}
                ],
                [
                    {"text": BTN_SERIES},
                    {"text": BTN_ANIME}
                ],
                [
                    {"text": BTN_NOTE},
                    {"text": BTN_CANCEL}
                ]
            ],
            "one_time_keyboard": true,
            "resize_keyboard": true
        })
    }

    pub fn status_keyboard(_knowledge_type: &KnowledgeType) -> serde_json::Value {
        serde_json::json!({
            "keyboard": [
                [
                    {"text": BTN_BACKLOG},
                    {"text": BTN_DONE}
                ],
                [
                    {"text": BTN_DROPPED},
                    {"text": BTN_CANCEL}
                ]
            ],
            "one_time_keyboard": true,
            "resize_keyboard": true
        })
    }

    pub fn confirm_ai_keyboard() -> serde_json::Value {
        serde_json::json!({
            "keyboard": [
                [{"text": "✅ Confirm"}],
                [
                    {"text": BTN_BOOK},
                    {"text": BTN_MOVIE}
                ],
                [
                    {"text": BTN_SERIES},
                    {"text": BTN_ANIME}
                ],
                [
                    {"text": BTN_NOTE},
                    {"text": BTN_CANCEL}
                ]
            ],
            "one_time_keyboard": true,
            "resize_keyboard": true
        })
    }

    pub fn skip_keyboard() -> serde_json::Value {
        serde_json::json!({
            "keyboard": [
                [{"text": BTN_SKIP}],
                [{"text": BTN_CANCEL}]
            ],
            "one_time_keyboard": true,
            "resize_keyboard": true
        })
    }

    pub fn remove_keyboard() -> serde_json::Value {
        serde_json::json!({
            "remove_keyboard": true
        })
    }
}

async fn send_telegram_api(url: &str, payload: &serde_json::Value) -> Result<()> {
    let headers = Headers::new();
    headers.set("Content-Type", "application/json")?;

    let mut req_init = RequestInit::new();
    req_init.with_method(Method::Post);
    req_init.with_headers(headers);
    req_init.with_body(Some(serde_json::to_string(payload)?.into()));

    let req = Request::new_with_init(url, &req_init)?;
    let mut resp = Fetch::Request(req).send().await?;
    if resp.status_code() != 200 {
        let err_text = resp.text().await?;
        crate::log_event!(
            "warn",
            "telegram.api.failed",
            "body_chars={}",
            err_text.chars().count()
        );
    }
    Ok(())
}