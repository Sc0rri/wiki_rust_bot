use serde::{Deserialize, Serialize};
use worker::*;

// Type buttons
pub const BTN_BOOK: &str = "📚 Book";
pub const BTN_MOVIE: &str = "🎬 Movie";
pub const BTN_SERIES: &str = "📺 Series";
pub const BTN_ANIME: &str = "🎌 Anime";
pub const BTN_ARTICLE: &str = "📄 Article";
pub const BTN_COURSE: &str = "🎓 Course";
pub const BTN_PAPER: &str = "📑 Paper";
pub const BTN_TOOL: &str = "🛠 Tool";
pub const BTN_PDF: &str = "📕 PDF";
pub const BTN_IMAGE: &str = "🖼 Image";
pub const BTN_IDEA: &str = "💡 Idea";
pub const BTN_NOTE: &str = "📝 Note";
pub const BTN_OTHER: &str = "📋 Other";

// Status buttons
pub const BTN_TO_READ: &str = "📋 To-read";
pub const BTN_READ: &str = "✅ Read";
pub const BTN_TO_WATCH: &str = "📋 To-watch";
pub const BTN_WATCHED: &str = "✅ Watched";
pub const BTN_PLANNED: &str = "📋 Planned";
pub const BTN_IN_PROGRESS: &str = "▶ In progress";
pub const BTN_FINISHED: &str = "✅ Finished";
pub const BTN_DROPPED: &str = "❌ Dropped";
pub const BTN_USING: &str = "⭐ Using";
pub const BTN_LIBRARY: &str = "📚 Library";
pub const BTN_INTERESTING: &str = "💡 Interesting";

// Category buttons
pub const BTN_PROGRAMMING: &str = "💻 Programming";
pub const BTN_NEWS: &str = "📰 News";
pub const BTN_CONCEPT: &str = "🧠 Concept";
pub const BTN_EDUCATION: &str = "📚 Education";
pub const BTN_GAMING: &str = "🎮 Gaming";
pub const BTN_ENTERTAINMENT: &str = "🎬 Entertainment";
pub const BTN_RESEARCH: &str = "🔬 Research";
pub const BTN_BOOK_CAT: &str = "📖 Book";
pub const BTN_MANUAL: &str = "📘 Manual";
pub const BTN_NOTES: &str = "📝 Notes";
pub const BTN_DOCUMENT: &str = "📄 Document";
pub const BTN_DIAGRAM: &str = "📊 Diagram";
pub const BTN_BOOK_COVER: &str = "📚 Book cover";

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
                    {"text": BTN_PAPER},
                    {"text": BTN_TOOL}
                ],
                [
                    {"text": BTN_PDF},
                    {"text": BTN_IMAGE}
                ],
                [
                    {"text": BTN_IDEA},
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

    pub fn category_keyboard(content_type: &crate::state::ContentType) -> serde_json::Value {
        let buttons: Vec<Vec<serde_json::Value>> = match content_type {
            crate::state::ContentType::Article => vec![
                vec![
                    serde_json::json!({"text": BTN_PROGRAMMING}),
                    serde_json::json!({"text": BTN_NEWS})
                ],
                vec![
                    serde_json::json!({"text": BTN_CONCEPT}),
                    serde_json::json!({"text": BTN_EDUCATION})
                ],
                vec![
                    serde_json::json!({"text": BTN_GAMING}),
                    serde_json::json!({"text": BTN_ENTERTAINMENT})
                ],
                vec![serde_json::json!({"text": BTN_OTHER})]
            ],
            crate::state::ContentType::Pdf => vec![
                vec![
                    serde_json::json!({"text": BTN_PROGRAMMING}),
                    serde_json::json!({"text": BTN_RESEARCH})
                ],
                vec![
                    serde_json::json!({"text": BTN_BOOK_CAT}),
                    serde_json::json!({"text": BTN_MANUAL})
                ],
                vec![serde_json::json!({"text": BTN_OTHER})]
            ],
            crate::state::ContentType::Image => vec![
                vec![
                    serde_json::json!({"text": BTN_BOOK_COVER}),
                    serde_json::json!({"text": BTN_NOTES})
                ],
                vec![
                    serde_json::json!({"text": BTN_DIAGRAM}),
                    serde_json::json!({"text": BTN_DOCUMENT})
                ],
                vec![serde_json::json!({"text": BTN_OTHER})]
            ],
            _ => vec![vec![serde_json::json!({"text": BTN_CANCEL})]]
        };

        let mut keyboard = serde_json::json!(buttons);
        keyboard
            .as_object_mut()
            .unwrap()
            .insert("one_time_keyboard".to_string(), serde_json::json!(true));
        keyboard
            .as_object_mut()
            .unwrap()
            .insert("resize_keyboard".to_string(), serde_json::json!(true));
        keyboard
    }

    pub fn status_keyboard(content_type: &crate::state::ContentType) -> serde_json::Value {
        let buttons: Vec<Vec<serde_json::Value>> = match content_type {
            crate::state::ContentType::Book => vec![
                vec![
                    serde_json::json!({"text": BTN_TO_READ}),
                    serde_json::json!({"text": BTN_READ})
                ],
                vec![
                    serde_json::json!({"text": BTN_DROPPED}),
                    serde_json::json!({"text": BTN_CANCEL})
                ]
            ],
            crate::state::ContentType::Movie | crate::state::ContentType::Series | crate::state::ContentType::Anime => vec![
                vec![
                    serde_json::json!({"text": BTN_TO_WATCH}),
                    serde_json::json!({"text": BTN_WATCHED})
                ],
                vec![
                    serde_json::json!({"text": BTN_DROPPED}),
                    serde_json::json!({"text": BTN_CANCEL})
                ]
            ],
            crate::state::ContentType::Course => vec![
                vec![
                    serde_json::json!({"text": BTN_PLANNED}),
                    serde_json::json!({"text": BTN_IN_PROGRESS}),
                    serde_json::json!({"text": BTN_FINISHED})
                ],
                vec![
                    serde_json::json!({"text": BTN_DROPPED}),
                    serde_json::json!({"text": BTN_CANCEL})
                ]
            ],
            crate::state::ContentType::Tool => vec![
                vec![
                    serde_json::json!({"text": BTN_USING}),
                    serde_json::json!({"text": BTN_LIBRARY}),
                    serde_json::json!({"text": BTN_INTERESTING})
                ],
                vec![serde_json::json!({"text": BTN_CANCEL})]
            ],
            _ => vec![vec![serde_json::json!({"text": BTN_CANCEL})]]
        };

        let mut keyboard = serde_json::json!(buttons);
        keyboard
            .as_object_mut()
            .unwrap()
            .insert("one_time_keyboard".to_string(), serde_json::json!(true));
        keyboard
            .as_object_mut()
            .unwrap()
            .insert("resize_keyboard".to_string(), serde_json::json!(true));
        keyboard
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
}