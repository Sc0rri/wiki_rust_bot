use crate::state::{ContentStatus, ContentType, PendingItem};
use crate::parser::ParserService;
use crate::get_env_or_secret;
use worker::*;

pub struct GitHubService;

impl GitHubService {
    pub async fn save_to_inbox(
        env: &Env,
        item: &PendingItem,
    ) -> Result<String> {
        let token = env.secret("GITHUB_TOKEN")?.to_string();
        let repo = get_env_or_secret(env, "GITHUB_REPO", "Sc0rri/wiki");
        
        let filename = ParserService::generate_filename(&item.title, &item.content_type, &item.status);
        let path = format!("inbox/{}", filename);
        
        let content = Self::generate_markdown(item);
        let content_base64 = base64::encode(&content);

        let url = format!(
            "https://api.github.com/repos/{}/contents/{}",
            repo, path
        );

        let payload = serde_json::json!({
            "message": format!("Add {}: {}", item.content_type.label().to_lowercase(), item.title),
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

    fn generate_markdown(item: &PendingItem) -> String {
        let mut md = String::new();

        md.push_str("---\n");
        md.push_str(&format!("type: {}\n", item.content_type.label().to_lowercase()));
        md.push_str(&format!("title: \"{}\"\n", item.title.replace('"', "\\\"")));
        
        if let Some(ref author) = item.author {
            md.push_str(&format!("author: \"{}\"\n", author.replace('"', "\\\"")));
        }
        
        if let Some(year) = item.year {
            md.push_str(&format!("year: {}\n", year));
        }
        
        if let Some(ref url) = item.url {
            md.push_str(&format!("url: \"{}\"\n", url));
        }
        
        md.push_str(&format!("status: {}\n", item.status.label().to_lowercase()));
        md.push_str(&format!("created: {}\n", chrono::Utc::now().format("%Y-%m-%d")));
        md.push_str("---\n\n");

        if let Some(ref desc) = item.description {
            md.push_str(desc);
            md.push('\n');
        }

        md
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_markdown_should_create_valid_frontmatter() {
        let item = PendingItem {
            title: "Test Book".to_string(),
            content_type: ContentType::Book,
            status: ContentStatus::Done,
            url: Some("https://example.com".to_string()),
            author: Some("Test Author".to_string()),
            year: Some(2024),
            description: Some("A test description".to_string()),
        };

        let md = GitHubService::generate_markdown(&item);
        
        assert!(md.contains("type: book"));
        assert!(md.contains("title: \"Test Book\""));
        assert!(md.contains("author: \"Test Author\""));
        assert!(md.contains("year: 2024"));
        assert!(md.contains("status: done"));
        assert!(md.contains("A test description"));
    }
}