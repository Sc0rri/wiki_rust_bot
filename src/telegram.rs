use serde::{Deserialize, Serialize};
use worker::*;

use crate::state::ContentStatus;
use crate::state::KnowledgeType;

// Type buttons
pub const BTN_BOOK: &str = "📚 Book";
pub const BTN_MOVIE: &str = "🎬 Movie";
pub const BTN_SERIES: &str = "📺 Series";
pub const BTN_ANIME: &str = "🎌 Anime";
pub const BTN_ARTICLE: &str = "📄 Article";
pub const BTN_COURSE: &str = "🎓 Course";
pub const BTN_TOOL: &str = "🛠 Tool";
pub const BTN_NOTE: &str = "📝 Note";
pub const BTN_OTHER: &str = "📋 Other";

// Status buttons
pub const BTN_BACKLOG: &str = "📋 Backlog";
pub const BTN_DONE: &str = "✅ Done";
pub const BTN_DROPPED: &str = "❌ Dropped";

// Common buttons
pub const BTN_CANCEL: &str = "❌ Cancel";
pub const BTN_CONFIRM: &str = "✅ Save";
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
                    {"text": BTN_ARTICLE},
                    {"text": BTN_COURSE}
                ],
                [
                    {"text": BTN_TOOL},
                    {"text": BTN_NOTE}
                ],
                [
                    {"text": BTN_OTHER}
                ]
            ],
            "one_time_keyboard": true,
            "resize_keyboard": true
        })
    }

    pub fn status_keyboard(knowledge_type: &KnowledgeType) -> serde_json::Value {
        let shelved_label = ContentStatus::Shelved.label(knowledge_type);
        let shelved_btn = format!("📚 {}", shelved_label);

        let mut buttons: Vec<Vec<serde_json::Value>> = vec![
            vec![
                serde_json::json!({"text": BTN_BACKLOG}),
                serde_json::json!({"text": BTN_DONE}),
            ],
            vec![
                serde_json::json!({"text": BTN_DROPPED}),
                serde_json::json!({"text": BTN_CANCEL}),
            ],
        ];

        // Add Shelved row for Tool and provider-based types
        if knowledge_type.has_status_options() && *knowledge_type != KnowledgeType::Course {
            buttons.insert(
                1,
                vec![serde_json::json!({"text": shelved_btn})],
            );
        }

        serde_json::json!({
            "keyboard": buttons,
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
                    {"text": BTN_ARTICLE},
                    {"text": BTN_COURSE}
                ],
                [
                    {"text": BTN_TOOL},
                    {"text": BTN_NOTE}
                ],
                [
                    {"text": BTN_OTHER},
                    {"text": BTN_CANCEL}
                ]
            ],
            "one_time_keyboard": true,
            "resize_keyboard": true
        })
    }

    pub fn confirm_keyboard() -> serde_json::Value {
        serde_json::json!({
            "keyboard": [
                [
                    {"text": BTN_CONFIRM},
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