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

        let mut item = PendingItem::new(name.to_string(), KnowledgeType::Link);
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

    /// YouTube's public oEmbed endpoint — no API key needed, and far more
    /// reliable than guessing a title from the URL (which for youtube.com
    /// is just the opaque video ID).
    pub async fn resolve_youtube(url: &str) -> Result<Option<(String, Option<String>)>> {
        let oembed_url = format!(
            "https://www.youtube.com/oembed?url={}&format=json",
            urlencoding::encode(url)
        );

        let mut req_init = RequestInit::new();
        req_init.with_method(Method::Get);
        let req = Request::new_with_init(&oembed_url, &req_init)?;
        let mut resp = Fetch::Request(req).send().await?;

        if resp.status_code() != 200 {
            return Ok(None);
        }

        let body: serde_json::Value = resp.json().await?;
        let title = body.get("title").and_then(|v| v.as_str()).map(|s| s.to_string());
        let author = body.get("author_name").and_then(|v| v.as_str()).map(|s| s.to_string());

        Ok(title.map(|t| (t, author)))
    }

    /// Generic fallback for any other web page: fetch the HTML and pull out
    /// <title> and a meta description. No AI involved — this is mechanical
    /// extraction, which is both cheaper and more reliable than asking a
    /// model to guess a page's title from a URL alone.
    pub async fn resolve_web_title(url: &str) -> Result<Option<(String, Option<String>)>> {
        let mut req_init = RequestInit::new();
        req_init.with_method(Method::Get);
        let req = Request::new_with_init(url, &req_init)?;
        let mut resp = match Fetch::Request(req).send().await {
            Ok(r) => r,
            Err(e) => {
                crate::log_event!("warn", "resolver.web.fetch_failed", "error={:?}", e);
                return Ok(None);
            }
        };

        if resp.status_code() != 200 {
            return Ok(None);
        }

        let html = resp.text().await?;
        // Title/description are always near the top of <head> — no need to
        // scan a whole large page.
        let snippet_len = html.len().min(80_000);
        let snippet = &html[..snippet_len];

        let title = Self::extract_tag_content(snippet, "title")
            .map(|t| Self::decode_html_entities(t.trim()))
            .filter(|t| !t.is_empty());
        let description = Self::extract_meta_description(snippet)
            .map(|d| Self::decode_html_entities(d.trim()))
            .filter(|d| !d.is_empty());

        Ok(title.map(|t| (t, description)))
    }

    fn extract_tag_content(html: &str, tag: &str) -> Option<String> {
        let lower = html.to_lowercase();
        let open_tag = format!("<{}", tag);
        let start = lower.find(&open_tag)?;
        let after_open = lower[start..].find('>')? + start + 1;
        let close_tag = format!("</{}>", tag);
        let end_rel = lower[after_open..].find(&close_tag)?;
        Some(html[after_open..after_open + end_rel].to_string())
    }

    fn extract_meta_description(html: &str) -> Option<String> {
        let lower = html.to_lowercase();
        for marker in ["name=\"description\"", "property=\"og:description\""] {
            if let Some(pos) = lower.find(marker) {
                let tag_start = lower[..pos].rfind("<meta")?;
                let tag_end = lower[pos..].find('>').map(|e| e + pos)?;
                let tag = &html[tag_start..tag_end];
                let tag_lower = tag.to_lowercase();
                if let Some(c_pos) = tag_lower.find("content=\"") {
                    let content_start = c_pos + "content=\"".len();
                    if let Some(end_rel) = tag[content_start..].find('"') {
                        return Some(tag[content_start..content_start + end_rel].to_string());
                    }
                }
            }
        }
        None
    }

    fn decode_html_entities(s: &str) -> String {
        s.replace("&amp;", "&")
            .replace("&quot;", "\"")
            .replace("&#39;", "'")
            .replace("&apos;", "'")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
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

    #[test]
    fn extract_tag_content_should_find_title() {
        let html = "<html><head><title>Statamic - Flat-file CMS</title></head></html>";
        assert_eq!(
            Resolver::extract_tag_content(html, "title"),
            Some("Statamic - Flat-file CMS".to_string())
        );
    }

    #[test]
    fn extract_meta_description_should_find_og_description() {
        let html = r#"<meta property="og:description" content="A simple, powerful CMS">"#;
        assert_eq!(
            Resolver::extract_meta_description(html),
            Some("A simple, powerful CMS".to_string())
        );
    }

    #[test]
    fn decode_html_entities_should_unescape_common_entities() {
        assert_eq!(Resolver::decode_html_entities("Tom &amp; Jerry"), "Tom & Jerry");
        assert_eq!(Resolver::decode_html_entities("&quot;quoted&quot;"), "\"quoted\"");
    }
}