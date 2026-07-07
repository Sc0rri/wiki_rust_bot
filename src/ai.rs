use crate::state::{KnowledgeType, PendingItem};
use crate::get_env_or_secret;
use serde::Deserialize;
use worker::*;

#[derive(Deserialize)]
struct AiAnalysis {
    #[serde(rename = "type")]
    content_type: String,
    title: Option<String>,
    author: Option<String>,
    year: Option<i32>,
    description: Option<String>,
    tags: Option<Vec<String>>,
}

pub struct AiService;

impl AiService {
    pub async fn analyze_content(
        env: &Env,
        text: &str,
    ) -> Result<Option<PendingItem>> {
        let prompt = format!(
            "Analyze the following text and determine what type of content it is.\n\
             Text: \"{}\"\n\
             Respond with JSON matching the required schema.",
            text
        );

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
                "json_schema": {
                    "type": "object",
                    "properties": {
                        "type": {
                            "type": "string",
                            "enum": ["book", "movie", "series", "anime", "article", "course", "tool", "note", "other"]
                        },
                        "title": { "type": "string" },
                        "author": { "type": "string" },
                        "year": { "type": "integer" },
                        "description": { "type": "string" },
                        "tags": {
                            "type": "array",
                            "items": { "type": "string" }
                        }
                    },
                    "required": ["type", "title"]
                }
            }
        });

        let result: Result<serde_json::Value> = ai.run(model, &input).await;
        let response_text = match result {
            Ok(value) => {
                if let Some(text) = value.get("response").and_then(|v| v.as_str()) {
                    text.to_string()
                } else {
                    return Ok(None);
                }
            }
            _ => return Ok(None),
        };

        // JSON Schema guarantees valid JSON — no manual parsing needed
        Self::parse_ai_response(&response_text, text)
    }

    fn parse_ai_response(response: &str, original_text: &str) -> Result<Option<PendingItem>> {
        let cleaned = response.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }

        let parsed: AiAnalysis = match serde_json::from_str(cleaned) {
            Ok(v) => v,
            Err(e) => {
                crate::log_event!("warn", "ai.analysis.json_parse_failed", "error={}", e);
                return Ok(None);
            }
        };

        let knowledge_type = match parsed.content_type.as_str() {
            "book" => KnowledgeType::Book,
            "movie" => KnowledgeType::Movie,
            "series" => KnowledgeType::Series,
            "anime" => KnowledgeType::Anime,
            "article" => KnowledgeType::Article,
            "course" => KnowledgeType::Course,
            "tool" => KnowledgeType::Tool,
            "note" => KnowledgeType::Note,
            _ => KnowledgeType::Other,
        };

        let mut item = PendingItem::new(
            parsed.title.unwrap_or_else(|| original_text.to_string()),
            knowledge_type,
        );
        item.author = parsed.author;
        item.year = parsed.year;
        item.description = parsed.description;
        item.tags = parsed.tags.unwrap_or_default();
        Ok(Some(item))
    }

    pub async fn enrich_url(
        env: &Env,
        url: &str,
        page_content: &str,
    ) -> Result<Option<PendingItem>> {
        let prompt = format!(
            "Analyze this webpage and extract content metadata.\n\
             URL: {}\n\
             Content preview: {}\n\
             Respond with JSON matching the required schema.",
            url,
            &page_content[..page_content.len().min(2000)]
        );

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
                "json_schema": {
                    "type": "object",
                    "properties": {
                        "type": {
                            "type": "string",
                            "enum": ["book", "movie", "series", "anime", "article", "course", "tool", "note", "other"]
                        },
                        "title": { "type": "string" },
                        "author": { "type": "string" },
                        "year": { "type": "integer" },
                        "description": { "type": "string" },
                        "tags": {
                            "type": "array",
                            "items": { "type": "string" }
                        }
                    },
                    "required": ["type", "title"]
                }
            }
        });

        let result: Result<serde_json::Value> = ai.run(model, &input).await;
        let response_text = match result {
            Ok(value) => {
                if let Some(text) = value.get("response").and_then(|v| v.as_str()) {
                    text.to_string()
                } else {
                    return Ok(None);
                }
            }
            _ => return Ok(None),
        };

        Self::parse_ai_response(&response_text, url)
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