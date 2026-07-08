use crate::state::{KnowledgeType, PendingItem, ResourceProvider};
use worker::*;

/// Resolves metadata for known providers using their public APIs (no AI).
pub struct Resolver;

impl Resolver {
    /// Fetch GitHub repo metadata: description, language, stars, topics
    pub async fn resolve_github(env: &Env, owner_repo: &str) -> Result<Option<PendingItem>> {
        let token = env.secret("GITHUB_TOKEN")
            .map(|s| s.to_string())
            .unwrap_or_default();

        let url = format!("https://api.github.com/repos/{}", owner_repo);

        let headers = Headers::new();
        headers.set("User-Agent", "wiki-rust-bot")?;
        headers.set("Accept", "application/vnd.github.v3+json")?;
        if !token.is_empty() {
            headers.set("Authorization", &format!("Bearer {}", token))?;
        }

        let mut req_init = RequestInit::new();
        req_init.with_method(Method::Get);
        req_init.with_headers(headers);

        let req = Request::new_with_init(&url, &req_init)?;
        let mut resp = Fetch::Request(req).send().await?;

        if resp.status_code() != 200 {
            return Ok(None);
        }

        let body: serde_json::Value = resp.json().await?;

        let name = body.get("name").and_then(|v| v.as_str()).unwrap_or(owner_repo);
        let description = body.get("description").and_then(|v| v.as_str());
        let language = body.get("language").and_then(|v| v.as_str());
        let stars = body.get("stargazers_count").and_then(|v| v.as_i64()).map(|s| s as i32);
        let topics: Vec<String> = body
            .get("topics")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|t| t.as_str().map(|s| s.to_string())).collect())
            .unwrap_or_default();

        let mut item = PendingItem::new(name.to_string(), KnowledgeType::Tool);
        item.provider = ResourceProvider::Github;
        item.description = description.map(|s| s.to_string());
        item.language = language.map(|s| s.to_string());
        item.stars = stars;
        item.tags = topics;

        crate::log_event!(
            "info",
            "resolver.github.success",
            "repo={} stars={} lang={:?}",
            owner_repo,
            stars.unwrap_or(0),
            language
        );

        Ok(Some(item))
    }

    /// Extract owner/repo from a GitHub URL
    pub fn parse_github_url(url: &str) -> Option<String> {
        let clean = url
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .trim_start_matches("www.");

        if !clean.starts_with("github.com/") {
            return None;
        }

        let path = clean.trim_start_matches("github.com/");
        let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

        if segments.len() >= 2 {
            Some(format!("{}/{}", segments[0], segments[1]))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_github_url_should_extract_owner_repo() {
        assert_eq!(
            Resolver::parse_github_url("https://github.com/tokio-rs/tokio"),
            Some("tokio-rs/tokio".to_string())
        );
        assert_eq!(
            Resolver::parse_github_url("https://github.com/serde-rs/serde"),
            Some("serde-rs/serde".to_string())
        );
        assert_eq!(
            Resolver::parse_github_url("https://github.com/rust-lang/rust/issues"),
            Some("rust-lang/rust".to_string())
        );
        assert_eq!(
            Resolver::parse_github_url("https://example.com"),
            None
        );
    }
}