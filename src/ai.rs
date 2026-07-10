use crate::state::{KnowledgeType, PendingItem};
use crate::get_env_or_secret;
use worker::*;

pub struct AiService;

/// Only the four media types are worth asking the model to distinguish —
/// anything else that arrives as plain text is a Note by elimination
/// (URLs never go through this path at all, they're always a Link).
fn content_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "type": { "type": "string", "enum": ["book", "movie", "series", "anime", "note"] },
            "title": { "type": "string" },
            "author": { "type": "string" },
            "year": { "type": "integer" },
            "description": { "type": "string" },
            "tags": { "type": "array", "items": { "type": "string" } }
        },
        "required": ["type", "title"]
    })
}

impl AiService {
    pub async fn analyze_content(
        env: &Env,
        text: &str,
    ) -> Result<Option<PendingItem>> {
        let prompt = format!(
            "Analyze the following text and determine what type of content it is.\n\nText: \"{}\"",
            text
        );

        let parsed = match Self::run_json(env, &prompt).await? {
            Some(v) => v,
            None => return Ok(None),
        };

        Ok(Some(Self::build_item(&parsed, text)))
    }

    /// Runs the model with JSON Mode enabled and returns the parsed JSON value.
    /// Handles both possible response shapes Workers AI can return: `response`
    /// as a structured object (JSON Mode honored) or as a string (some models
    /// still return text even when a schema is requested) — treating only the
    /// string case as valid was a likely source of intermittent "AI doesn't
    /// work" failures.
    async fn run_json(env: &Env, prompt: &str) -> Result<Option<serde_json::Value>> {
        let ai = match env.ai("AI") {
            Ok(ai) => ai,
            Err(e) => {
                crate::log_event!("error", "ai.init.failed", "error={:?}", e);
                return Ok(None);
            }
        };

        let model = get_env_or_secret(env, "AI_MODEL", "@cf/meta/llama-3.1-8b-instruct-fp8-fast");

        let input = serde_json::json!({
            "messages": [{"role": "user", "content": prompt}],
            "temperature": 0.15,
            "response_format": {
                "type": "json_schema",
                "json_schema": content_schema()
            }
        });

        let result: Result<serde_json::Value> = ai.run(model, &input).await;
        let value = match result {
            Ok(v) => v,
            Err(e) => {
                crate::log_event!("warn", "ai.analysis.request_failed", "error={:?}", e);
                return Ok(None);
            }
        };

        match value.get("response") {
            Some(obj) if obj.is_object() => Ok(Some(obj.clone())),
            Some(serde_json::Value::String(text)) => Ok(Self::extract_json(text)),
            _ => Ok(None),
        }
    }

    /// Defensive fallback for models that ignore response_format and return a
    /// text blob (possibly wrapped in markdown fences or commentary).
    fn extract_json(response: &str) -> Option<serde_json::Value> {
        let cleaned = response.trim();
        if cleaned.is_empty() || cleaned == "null" {
            return None;
        }
        let json_start = cleaned.find('{')?;
        let json_end = cleaned.rfind('}')?;
        match serde_json::from_str(&cleaned[json_start..=json_end]) {
            Ok(v) => Some(v),
            Err(e) => {
                crate::log_event!("warn", "ai.analysis.json_parse_failed", "error={}", e);
                None
            }
        }
    }

    fn build_item(parsed: &serde_json::Value, fallback_title: &str) -> PendingItem {
        let knowledge_type = match parsed.get("type").and_then(|v| v.as_str()) {
            Some("book") => KnowledgeType::Book,
            Some("movie") => KnowledgeType::Movie,
            Some("series") => KnowledgeType::Series,
            Some("anime") => KnowledgeType::Anime,
            _ => KnowledgeType::Note,
        };

        let title = parsed
            .get("title")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or(fallback_title)
            .to_string();

        let author = parsed.get("author").and_then(|v| v.as_str()).map(|s| s.to_string());
        let year = parsed.get("year").and_then(|v| v.as_i64()).map(|y| y as i32);
        let description = parsed.get("description").and_then(|v| v.as_str()).map(|s| s.to_string());
        let tags: Vec<String> = parsed
            .get("tags")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|t| t.as_str().map(|s| s.to_string())).collect())
            .unwrap_or_default();

        let mut item = PendingItem::new(title, knowledge_type);
        item.author = author;
        item.year = year;
        item.description = description;
        item.tags = tags;
        item
    }

    /// Format a preview of the AI analysis for display to the user
    pub fn format_preview(item: &PendingItem) -> String {
        let mut preview = format!("🤖 Looks like: {} {}\n", item.knowledge_type.emoji(), item.title);
        if let Some(ref author) = item.author {
            preview.push_str(&format!("   👤 {}\n", author));
        }
        if let Some(year) = item.year {
            preview.push_str(&format!("   📅 {}\n", year));
        }
        if let Some(ref desc) = item.description {
            if desc.len() < 120 {
                preview.push_str(&format!("   📝 {}\n", desc));
            }
        }
        preview.push_str("\nConfirm or change type?");
        preview
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_item_should_map_known_type() {
        let parsed = serde_json::json!({"type": "book", "title": "Sapiens", "author": "Harari", "year": 2011});
        let item = AiService::build_item(&parsed, "fallback");
        assert_eq!(item.knowledge_type, KnowledgeType::Book);
        assert_eq!(item.title, "Sapiens");
        assert_eq!(item.author.as_deref(), Some("Harari"));
        assert_eq!(item.year, Some(2011));
    }

    #[test]
    fn build_item_should_fall_back_to_note_for_unknown_type() {
        let parsed = serde_json::json!({"type": "banana", "title": "Something"});
        let item = AiService::build_item(&parsed, "fallback");
        assert_eq!(item.knowledge_type, KnowledgeType::Note);
    }

    #[test]
    fn build_item_should_use_fallback_title_when_missing() {
        let parsed = serde_json::json!({"type": "note"});
        let item = AiService::build_item(&parsed, "original text");
        assert_eq!(item.title, "original text");
    }

    #[test]
    fn extract_json_should_pull_object_from_wrapped_text() {
        let text = "Here you go:\n```json\n{\"type\": \"note\", \"title\": \"X\"}\n```";
        let parsed = AiService::extract_json(text).expect("should parse");
        assert_eq!(parsed.get("title").and_then(|v| v.as_str()), Some("X"));
    }

    #[test]
    fn extract_json_should_return_none_for_empty() {
        assert!(AiService::extract_json("").is_none());
        assert!(AiService::extract_json("null").is_none());
    }
}
