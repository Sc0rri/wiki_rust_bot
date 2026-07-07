use crate::state::{ContentStatus, ContentType, PendingItem};
use crate::get_env_or_secret;
use worker::*;

pub struct AiService;

impl AiService {
    pub async fn analyze_content(
        env: &Env,
        text: &str,
    ) -> Result<Option<PendingItem>> {
        let prompt = format!(
            r#"Analyze the following text and determine what type of content it is.
Possible types: book, movie, series, anime, article, course, paper, tool, pdf, image, idea, note, other.

Text: "{}"

Respond ONLY with valid JSON in this exact format:
{{
  "type": "book|movie|series|anime|article|course|paper|tool|pdf|image|idea|note|other",
  "title": "Title here",
  "author": "Author or director if identifiable",
  "year": 2024,
  "category": "category if applicable (for article/pdf/image)",
  "description": "brief 1-2 sentence description"
}}

If you cannot determine the type, respond with: {{"type": "other", "title": "{}"}}"#,
            text, text
        );

        let ai = match env.ai("AI") {
            Ok(ai) => ai,
            Err(e) => {
                crate::log_event!("error", "ai.init.failed", "error={:?}", e);
                return Ok(None);
            }
        };
        
        let model = get_env_or_secret(env, "AI_MODEL", "@cf/meta/llama-3.2-11b-instruct");

        let input = serde_json::json!({
            "messages": [
                {
                    "role": "user",
                    "content": prompt
                }
            ]
        });

        let result: Result<serde_json::Value> = ai.run(model, &input).await;
        let response_text = match result {
            Ok(value) => {
                if let Some(text) = value.get("response").and_then(|v| v.as_str()) {
                    text.to_string()
                } else if let Some(text) = value.as_str() {
                    text.to_string()
                } else {
                    crate::log_event!("warn", "ai.analysis.unexpected_format");
                    return Ok(None);
                }
            }
            Err(e) => {
                crate::log_event!("error", "ai.analysis.failed", "error={:?}", e);
                return Ok(None);
            }
        };

        crate::log_event!(
            "info",
            "ai.analysis.response",
            "text_chars={}",
            response_text.chars().count()
        );

        Self::parse_ai_response(&response_text, text)
    }

    fn parse_ai_response(response: &str, original_text: &str) -> Result<Option<PendingItem>> {
        let cleaned = response.trim();
        
        if cleaned == "null" || cleaned.is_empty() {
            return Ok(None);
        }

        let json_start = cleaned.find('{');
        let json_end = cleaned.rfind('}');
        
        let json_str = if let (Some(start), Some(end)) = (json_start, json_end) {
            &cleaned[start..=end]
        } else {
            cleaned
        };

        let parsed: serde_json::Value = match serde_json::from_str(json_str) {
            Ok(v) => v,
            Err(e) => {
                crate::log_event!("warn", "ai.analysis.json_parse_failed", "error={}", e);
                return Ok(None);
            }
        };

        let content_type = match parsed.get("type").and_then(|v| v.as_str()) {
            Some("book") => ContentType::Book,
            Some("movie") => ContentType::Movie,
            Some("series") => ContentType::Series,
            Some("anime") => ContentType::Anime,
            Some("article") => ContentType::Article,
            Some("course") => ContentType::Course,
            Some("paper") => ContentType::Paper,
            Some("tool") => ContentType::Tool,
            Some("pdf") => ContentType::Pdf,
            Some("image") => ContentType::Image,
            Some("idea") => ContentType::Idea,
            Some("note") => ContentType::Note,
            _ => ContentType::Other,
        };

        let title = parsed
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or(original_text)
            .to_string();

        let author = parsed.get("author").and_then(|v| v.as_str()).map(|s| s.to_string());
        
        let year = parsed
            .get("year")
            .and_then(|v| v.as_i64())
            .map(|y| y as i32);

        let category = parsed.get("category").and_then(|v| v.as_str()).map(|s| s.to_string());
        
        let description = parsed
            .get("description")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        Ok(Some(PendingItem {
            title,
            content_type,
            status: ContentStatus::ToRead,
            category,
            url: None,
            author,
            year,
            description,
            tags: Vec::new(),
            source: String::new(),
            processed: false,
        }))
    }

    pub async fn analyze_url(
        env: &Env,
        url: &str,
        page_content: &str,
    ) -> Result<Option<PendingItem>> {
        let prompt = format!(
            r#"Analyze this webpage and extract content metadata.
URL: {}
Content preview: {}

Respond ONLY with valid JSON:
{{
  "type": "book|movie|series|anime|article|course|paper|tool|pdf|image|idea|note|other",
  "title": "Title",
  "author": "Author/director",
  "year": 2024,
  "category": "category if applicable",
  "description": "brief description"
}}"#,
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
        
        let model = get_env_or_secret(env, "AI_MODEL", "@cf/meta/llama-3.2-11b-instruct");

        let input = serde_json::json!({
            "messages": [
                {
                    "role": "user",
                    "content": prompt
                }
            ]
        });

        let result: Result<serde_json::Value> = ai.run(model, &input).await;
        let response_text = match result {
            Ok(value) => {
                if let Some(text) = value.get("response").and_then(|v| v.as_str()) {
                    text.to_string()
                } else if let Some(text) = value.as_str() {
                    text.to_string()
                } else {
                    return Ok(None);
                }
            }
            _ => return Ok(None),
        };

        Self::parse_ai_response(&response_text, url)
    }
}