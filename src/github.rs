use crate::state::PendingItem;
use crate::parser::ParserService;
use crate::get_env_or_secret;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use worker::*;

pub struct GitHubService;

impl GitHubService {
    /// Commits a binary file (photo/PDF) into inbox/assets/, returning the
    /// committed path. Unlike a Telegram file_id (which can expire), this is
    /// a permanent copy living in the private repo.
    pub async fn save_asset(env: &Env, filename: &str, bytes: &[u8]) -> Result<String> {
        let token = env.secret("GITHUB_TOKEN")?.to_string();
        let repo = get_env_or_secret(env, "GITHUB_REPO", "Sc0rri/wiki");

        let path = format!("inbox/assets/{}", filename);
        let content_base64 = STANDARD.encode(bytes);

        let url = format!("https://api.github.com/repos/{}/contents/{}", repo, path);

        let payload = serde_json::json!({
            "message": format!("Add asset: {}", filename),
            "content": content_base64,
            "branch": "main"
        });

        let headers = Headers::new();
        headers.set("Authorization", &format!("Bearer {}", token))?;
        headers.set("Content-Type", "application/json")?;
        headers.set("User-Agent", "wiki-rust-bot")?;

        let mut req_init = RequestInit::new();
        req_init.with_method(Method::Put);
        req_init.with_headers(headers);
        req_init.with_body(Some(serde_json::to_string(&payload)?.into()));

        let req = Request::new_with_init(&url, &req_init)?;
        let mut resp = Fetch::Request(req).send().await?;

        if resp.status_code() != 201 && resp.status_code() != 200 {
            let err_text = resp.text().await?;
            crate::log_event!(
                "error",
                "github.asset.failed",
                "status={} body={}",
                resp.status_code(),
                err_text.chars().count()
            );
            return Err(worker::Error::from(format!(
                "GitHub API error: {}",
                err_text.chars().take(200).collect::<String>()
            )));
        }

        Ok(path)
    }

    pub async fn save_to_inbox(
        env: &Env,
        item: &PendingItem,
    ) -> Result<String> {
        let token = env.secret("GITHUB_TOKEN")?.to_string();
        let repo = get_env_or_secret(env, "GITHUB_REPO", "Sc0rri/wiki");
        
        let filename = ParserService::generate_filename(item);
        let path = format!("inbox/pending/{}", filename);
        
        let content = Self::generate_yaml(item);
        let content_base64 = STANDARD.encode(&content);

        let url = format!(
            "https://api.github.com/repos/{}/contents/{}",
            repo, path
        );

        let payload = serde_json::json!({
            "message": format!("Add {}: {}", item.knowledge_type.label().to_lowercase(), item.title),
            "content": content_base64,
            "branch": "main"
        });

        let headers = Headers::new();
        headers.set("Authorization", &format!("Bearer {}", token))?;
        headers.set("Content-Type", "application/json")?;
        headers.set("User-Agent", "wiki-rust-bot")?;

        let mut req_init = RequestInit::new();
        req_init.with_method(Method::Put);
        req_init.with_headers(headers);
        req_init.with_body(Some(serde_json::to_string(&payload)?.into()));

        let req = Request::new_with_init(&url, &req_init)?;
        let mut resp = Fetch::Request(req).send().await?;

        if resp.status_code() != 201 && resp.status_code() != 200 {
            let err_text = resp.text().await?;
            crate::log_event!(
                "error",
                "github.commit.failed",
                "status={} body={}",
                resp.status_code(),
                err_text.chars().count()
            );
            return Err(worker::Error::from(format!(
                "GitHub API error: {}",
                err_text.chars().take(200).collect::<String>()
            )));
        }

        crate::log_event!(
            "info",
            "github.commit.success",
            "path={}",
            path
        );

        Ok(path)
    }

    /// Writes a user's answer to a clarifying question the compiler asked,
    /// as inbox/pending/<id>.reply.yaml — same GitHub Contents API pattern
    /// as save_to_inbox, just a different, simpler payload.
    pub async fn save_reply_to_inbox(env: &Env, item_id: &str, reply_text: &str) -> Result<String> {
        let token = env.secret("GITHUB_TOKEN")?.to_string();
        let repo = get_env_or_secret(env, "GITHUB_REPO", "Sc0rri/wiki");

        let path = format!("inbox/pending/{}.reply.yaml", item_id);
        let now = chrono::Utc::now().to_rfc3339();
        let content = format!(
            "---\nreplies_to: {}\ntext: \"{}\"\ncreated: {}\n---\n",
            item_id,
            Self::yaml_quote(reply_text),
            now
        );
        let content_base64 = STANDARD.encode(&content);

        let url = format!("https://api.github.com/repos/{}/contents/{}", repo, path);

        let payload = serde_json::json!({
            "message": format!("Clarification reply for {}", item_id),
            "content": content_base64,
            "branch": "main"
        });

        let headers = Headers::new();
        headers.set("Authorization", &format!("Bearer {}", token))?;
        headers.set("Content-Type", "application/json")?;
        headers.set("User-Agent", "wiki-rust-bot")?;

        let mut req_init = RequestInit::new();
        req_init.with_method(Method::Put);
        req_init.with_headers(headers);
        req_init.with_body(Some(serde_json::to_string(&payload)?.into()));

        let req = Request::new_with_init(&url, &req_init)?;
        let mut resp = Fetch::Request(req).send().await?;

        if resp.status_code() != 201 && resp.status_code() != 200 {
            let err_text = resp.text().await?;
            crate::log_event!(
                "error",
                "github.reply.failed",
                "status={} body_chars={}",
                resp.status_code(),
                err_text.chars().count()
            );
            return Err(worker::Error::from(format!(
                "GitHub API error: {}",
                err_text.chars().take(200).collect::<String>()
            )));
        }

        crate::log_event!(
            "info",
            "github.reply.success",
            "path={}",
            path
        );

        Ok(path)
    }

    /// Appends a log line to inbox/logs/<date>.log in the GitHub repo.
    /// Uses GET (to fetch existing content) + PUT (to overwrite with appended
    /// line). Best-effort: if GET fails (e.g. first write of the day) it
    /// starts a new file; if a concurrent write races, one may be lost —
    /// acceptable for debug logs.
    pub async fn append_log(env: &Env, level: &str, name: &str, message: &str) {
        let token = match env.secret("GITHUB_TOKEN") {
            Ok(t) => t.to_string(),
            Err(_) => return,
        };
        let repo = get_env_or_secret(env, "GITHUB_REPO", "Sc0rri/wiki");

        let date = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let path = format!("inbox/logs/{}.log", date);
        let url = format!("https://api.github.com/repos/{}/contents/{}", repo, path);

        let timestamp = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
        let new_line = format!("{} [{}] {} - {}\n", timestamp, level, name, message);

        // Try to GET existing file content (SHA + base64 body).
        let get_headers = Headers::new();
        let _ = get_headers.set("Authorization", &format!("Bearer {}", token));
        let _ = get_headers.set("User-Agent", "wiki-rust-bot");

        let mut get_req_init = RequestInit::new();
        get_req_init.with_method(Method::Get);
        get_req_init.with_headers(get_headers);

        let (existing_sha, existing_body) = if let Ok(req) = Request::new_with_init(&url, &get_req_init) {
            if let Ok(mut resp) = Fetch::Request(req).send().await {
                if resp.status_code() == 200 {
                    if let Ok(val) = resp.json::<serde_json::Value>().await {
                        let sha = val.get("sha").and_then(|s| s.as_str()).map(|s| s.to_string());
                        let content = val.get("content")
                            .and_then(|c| c.as_str())
                            .map(|c| c.replace('\n', "").replace('\r', ""))
                            .and_then(|c| {
                                use base64::Engine;
                                base64::engine::general_purpose::STANDARD.decode(c).ok()
                            })
                            .and_then(|bytes| String::from_utf8(bytes).ok())
                            .unwrap_or_default();
                        (sha, content)
                    } else {
                        (None, String::new())
                    }
                } else {
                    (None, String::new())
                }
            } else {
                (None, String::new())
            }
        } else {
            (None, String::new())
        };

        let new_content = format!("{}{}", existing_body, new_line);
        let content_base64 = base64::engine::general_purpose::STANDARD.encode(&new_content);

        let mut payload = serde_json::json!({
            "message": format!("Log: {}", name),
            "content": content_base64,
            "branch": "main"
        });
        if let Some(sha) = existing_sha {
            payload.as_object_mut().unwrap().insert("sha".to_string(), serde_json::Value::String(sha));
        }

        let put_headers = Headers::new();
        let _ = put_headers.set("Authorization", &format!("Bearer {}", token));
        let _ = put_headers.set("Content-Type", "application/json");
        let _ = put_headers.set("User-Agent", "wiki-rust-bot");

        let mut put_req_init = RequestInit::new();
        put_req_init.with_method(Method::Put);
        put_req_init.with_headers(put_headers);
        if let Ok(body) = serde_json::to_string(&payload) {
            put_req_init.with_body(Some(body.into()));
        }

        if let Ok(req) = Request::new_with_init(&url, &put_req_init) {
            let _ = Fetch::Request(req).send().await;
        }
    }

    fn yaml_quote(s: &str) -> String {
        s.replace('\\', "\\\\")   // backslash first — order matters
            .replace('"', "\\\"")
            .replace('\r', "\\r")
            .replace('\n', "\\n")
            .replace('\t', "\\t")
    }

    fn generate_yaml(item: &PendingItem) -> String {
        let mut yaml = String::new();

        yaml.push_str("---\n");
        yaml.push_str(&format!("id: {}\n", item.id));
        yaml.push_str(&format!("created: {}\n", item.created));
        yaml.push_str(&format!("source: {}\n", item.source));
        yaml.push_str(&format!("provider: {}\n", item.provider.label().to_lowercase()));
        yaml.push_str(&format!("chat_id: {}\n", item.chat_id));
        
        if let Some(ref url) = item.url {
            yaml.push_str(&format!("url: \"{}\"\n", Self::yaml_quote(url)));
        }
        
        yaml.push_str(&format!("type: {}\n", item.knowledge_type.label().to_lowercase()));
        yaml.push_str(&format!("status: {}\n", item.status.label(&item.knowledge_type).to_lowercase()));
        yaml.push_str(&format!("title: \"{}\"\n", Self::yaml_quote(&item.title)));

        if let Some(ref raw) = item.raw_text {
            if raw != &item.title {
                yaml.push_str(&format!("raw_text: \"{}\"\n", Self::yaml_quote(raw)));
            }
        }
        
        if let Some(ref author) = item.author {
            yaml.push_str(&format!("author: \"{}\"\n", Self::yaml_quote(author)));
        }
        
        if let Some(ref language) = item.language {
            yaml.push_str(&format!("language: {}\n", language));
        }
        
        if let Some(year) = item.year {
            yaml.push_str(&format!("year: {}\n", year));
        }
        
        if let Some(season) = item.season {
            yaml.push_str(&format!("season: {}\n", season));
        }
        
        if let Some(stars) = item.stars {
            yaml.push_str(&format!("stars: {}\n", stars));
        }
        
        if let Some(rating) = item.rating {
            yaml.push_str(&format!("rating: {}\n", rating));
        }
        
        if let Some(ref comment) = item.comment {
            yaml.push_str(&format!("comment: \"{}\"\n", Self::yaml_quote(comment)));
        }
        
        if !item.tags.is_empty() {
            yaml.push_str("tags:\n");
            for tag in &item.tags {
                yaml.push_str(&format!("  - \"{}\"\n", Self::yaml_quote(tag)));
            }
        } else {
            yaml.push_str("tags: []\n");
        }

        if let Some(ref mime) = item.asset_mime {
            yaml.push_str(&format!("asset_mime: {}\n", mime));
        }
        if let (Some(w), Some(h)) = (item.asset_width, item.asset_height) {
            yaml.push_str(&format!("asset_width: {}\n", w));
            yaml.push_str(&format!("asset_height: {}\n", h));
        }
        if let Some(ref sha) = item.asset_sha256 {
            yaml.push_str(&format!("asset_sha256: {}\n", sha));
        }
        
        yaml.push_str("---\n");

        yaml
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{ContentStatus, KnowledgeType, ResourceProvider};

    #[test]
    fn generate_yaml_should_create_valid_frontmatter() {
        let mut item = PendingItem::new("Test Article".to_string(), KnowledgeType::Link, 12345);
        item.author = Some("Test Author".to_string());
        item.year = Some(2024);
        item.status = ContentStatus::Backlog;
        item.provider = ResourceProvider::Web;
        item.tags = vec!["rust".to_string(), "wasm".to_string()];

        let yaml = GitHubService::generate_yaml(&item);
        
        assert!(yaml.contains("type: link"));
        assert!(yaml.contains("title: \"Test Article\""));
        assert!(yaml.contains("author: \"Test Author\""));
        assert!(yaml.contains("year: 2024"));
        assert!(yaml.contains("status: backlog"));
        assert!(yaml.contains("source: telegram"));
        assert!(yaml.contains("provider: web"));
        assert!(yaml.contains("chat_id: 12345"));
        assert!(yaml.contains("tags:"));
        assert!(yaml.contains("- \"rust\""));
        assert!(yaml.contains("id: "));
        assert!(yaml.contains("created: "));
        assert!(yaml.ends_with("---\n"));
    }

    #[test]
    fn generate_yaml_should_have_empty_tags_array() {
        let item = PendingItem::new("No Tags".to_string(), KnowledgeType::Book, 12345);
        let yaml = GitHubService::generate_yaml(&item);
        assert!(yaml.contains("tags: []\n"));
    }
}