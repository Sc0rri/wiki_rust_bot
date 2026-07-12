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

    fn generate_yaml(item: &PendingItem) -> String {
        let mut yaml = String::new();

        yaml.push_str("---\n");
        yaml.push_str(&format!("id: {}\n", item.id));
        yaml.push_str(&format!("created: {}\n", item.created));
        yaml.push_str(&format!("source: {}\n", item.source));
        yaml.push_str(&format!("provider: {}\n", item.provider.label().to_lowercase()));
        
        if let Some(ref url) = item.url {
            yaml.push_str(&format!("url: \"{}\"\n", url));
        }
        
        yaml.push_str(&format!("type: {}\n", item.knowledge_type.label().to_lowercase()));
        yaml.push_str(&format!("status: {}\n", item.status.label(&item.knowledge_type).to_lowercase()));
        yaml.push_str(&format!("title: \"{}\"\n", item.title.replace('"', "\\\"")));

        if let Some(ref raw) = item.raw_text {
            if raw != &item.title {
                yaml.push_str(&format!("raw_text: \"{}\"\n", raw.replace('"', "\\\"")));
            }
        }
        
        if let Some(ref author) = item.author {
            yaml.push_str(&format!("author: \"{}\"\n", author.replace('"', "\\\"")));
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
            yaml.push_str(&format!("comment: \"{}\"\n", comment.replace('"', "\\\"")));
        }
        
        if !item.tags.is_empty() {
            yaml.push_str("tags:\n");
            for tag in &item.tags {
                yaml.push_str(&format!("  - \"{}\"\n", tag));
            }
        } else {
            yaml.push_str("tags: []\n");
        }
        
        yaml.push_str(&format!("processed: {}\n", item.processed));
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
        let mut item = PendingItem::new("Test Article".to_string(), KnowledgeType::Link);
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
        assert!(yaml.contains("tags:"));
        assert!(yaml.contains("- \"rust\""));
        assert!(yaml.contains("processed: false"));
        assert!(yaml.contains("id: "));
        assert!(yaml.contains("created: "));
        assert!(yaml.ends_with("---\n"));
    }

    #[test]
    fn generate_yaml_should_have_empty_tags_array() {
        let item = PendingItem::new("No Tags".to_string(), KnowledgeType::Book);
        let yaml = GitHubService::generate_yaml(&item);
        assert!(yaml.contains("tags: []\n"));
    }
}