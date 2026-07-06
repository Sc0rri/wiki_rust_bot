use serde::{Deserialize, Serialize};
use worker::*;

pub const BTN_BOOK: &str = "📚 Book";
pub const BTN_MOVIE: &str = "🎬 Movie";
pub const BTN_SERIES: &str = "📺 Series";
pub const BTN_ANIME: &str = "🎌 Anime";
pub const BTN_OTHER: &str = "📋 Other";
pub const BTN_DONE: &str = "✅ Done";
pub const BTN_TO_READ: &str = "📋 To-read";
pub const BTN_TO_WATCH: &str = "📋 To-watch";
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
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct CallbackQuery {
    pub id: String,
    pub from: User,
    pub message: Option<Message>,
    pub data: Option<String>,
}

#[derive(Deserialize)]
struct GetFileResponse {
    ok: bool,
    result: Option<GetFileResult>,
}

#[derive(Deserialize)]
struct GetFileResult {
    file_path: Option<String>,
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

        let headers = Headers::new();
        headers.set("Content-Type", "application/json")?;

        let mut req_init = RequestInit::new();
        req_init.with_method(Method::Post);
        req_init.with_headers(headers);
        req_init.with_body(Some(serde_json::to_string(&payload)?.into()));

        let req = Request::new_with_init(&url, &req_init)?;
        let mut resp = Fetch::Request(req).send().await?;
        if resp.status_code() != 200 {
            let err_text = resp.text().await?;
            crate::log_event!(
                "warn",
                "telegram.send_message.failed",
                "body_chars={}",
                err_text.chars().count()
            );
        }
        Ok(())
    }

    pub async fn send_inline_message(
        bot_token: &str,
        chat_id: i64,
        text: &str,
        inline_markup: serde_json::Value,
    ) -> Result<()> {
        let url = format!("https://api.telegram.org/bot{}/sendMessage", bot_token);
        let payload = serde_json::json!({
            "chat_id": chat_id,
            "text": text,
            "reply_markup": inline_markup,
        });

        let headers = Headers::new();
        headers.set("Content-Type", "application/json")?;

        let mut req_init = RequestInit::new();
        req_init.with_method(Method::Post);
        req_init.with_headers(headers);
        req_init.with_body(Some(serde_json::to_string(&payload)?.into()));

        let req = Request::new_with_init(&url, &req_init)?;
        let mut resp = Fetch::Request(req).send().await?;
        if resp.status_code() != 200 {
            let err_text = resp.text().await?;
            crate::log_event!(
                "warn",
                "telegram.send_inline_message.failed",
                "body_chars={}",
                err_text.chars().count()
            );
        }
        Ok(())
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
                    {"text": BTN_OTHER},
                    {"text": BTN_CANCEL}
                ]
            ],
            "one_time_keyboard": true,
            "resize_keyboard": true
        })
    }

    pub fn status_keyboard(content_type: &crate::state::ContentType) -> serde_json::Value {
        let (btn1, btn2) = match content_type {
            crate::state::ContentType::Book => (BTN_DONE, BTN_TO_READ),
            _ => (BTN_DONE, BTN_TO_WATCH),
        };

        serde_json::json!({
            "keyboard": [
                [
                    {"text": btn1},
                    {"text": btn2}
                ],
                [
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

    pub fn details_keyboard() -> serde_json::Value {
        serde_json::json!({
            "keyboard": [
                [
                    {"text": BTN_SKIP},
                    {"text": BTN_CANCEL}
                ]
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

    pub async fn answer_callback_query(
        bot_token: &str,
        callback_query_id: &str,
        text: Option<&str>,
    ) -> Result<()> {
        let url = format!(
            "https://api.telegram.org/bot{}/answerCallbackQuery",
            bot_token
        );
        let mut payload = serde_json::json!({
            "callback_query_id": callback_query_id,
        });
        if let Some(t) = text {
            payload
                .as_object_mut()
                .unwrap()
                .insert("text".to_string(), serde_json::Value::String(t.to_string()));
        }

        let headers = Headers::new();
        headers.set("Content-Type", "application/json")?;

        let mut req_init = RequestInit::new();
        req_init.with_method(Method::Post);
        req_init.with_headers(headers);
        req_init.with_body(Some(serde_json::to_string(&payload)?.into()));

        let req = Request::new_with_init(&url, &req_init)?;
        let mut resp = Fetch::Request(req).send().await?;
        if resp.status_code() != 200 {
            let err_text = resp.text().await?;
            crate::log_event!(
                "warn",
                "telegram.answer_callback_query.failed",
                "body_chars={}",
                err_text.chars().count()
            );
        }
        Ok(())
    }

    pub async fn download_file(bot_token: &str, file_path: &str) -> Result<Vec<u8>> {
        let url = format!(
            "https://api.telegram.org/file/bot{}/{}",
            bot_token, file_path
        );

        let req = Request::new(&url, Method::Get)?;
        let mut resp = Fetch::Request(req).send().await?;

        if resp.status_code() != 200 {
            let err_text = resp.text().await?;
            return Err(worker::Error::from(format!(
                "download file failed: {}",
                err_text
            )));
        }

        let bytes = resp.bytes().await?;
        Ok(bytes)
    }
}