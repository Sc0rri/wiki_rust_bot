use crate::state::{KnowledgeType, PendingItem, ResourceProvider};
use worker::*;

/// Resolves metadata for known providers using their public APIs (no AI).
///
/// Currently supported resolutions:
/// - GitHub repos via the GitHub REST API (description, language, stars, topics)
/// - YouTube videos via the public oEmbed endpoint (title, author)
/// - Generic web pages via HTML `<title>` / meta / Open Graph tag extraction
///
/// Falls back gracefully (returns `None` / `Ok(None)`) when a provider is
/// unreachable or has no useful metadata, rather than failing the whole save flow.
pub struct Resolver;

impl Resolver {
    /// Fetch GitHub repo metadata: description, language, stars, topics
    pub async fn resolve_github(env: &Env, owner_repo: &str) -> Result<Option<PendingItem>> {
        let token = env
            .secret("GITHUB_TOKEN")
            .map(|s| s.to_string())
            .unwrap_or_default();

        let url = format!("https://api.github.com/repos/{}", owner_repo);

        let headers = Headers::new();
        headers.set(
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36",
        )?;
        headers.set("Accept-Language", "ru-RU,ru;q=0.9,en;q=0.8")?;
        headers.set("Accept-Encoding", "gzip, deflate, br")?;
        headers.set("Cache-Control", "no-cache")?;
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

        let name = body
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or(owner_repo);
        let description = body.get("description").and_then(|v| v.as_str());
        let language = body.get("language").and_then(|v| v.as_str());
        let stars = body
            .get("stargazers_count")
            .and_then(|v| v.as_i64())
            .map(|s| s as i32);
        let topics: Vec<String> = body
            .get("topics")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|t| t.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        let mut item = PendingItem::new(name.to_string(), KnowledgeType::Link, 0);
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
        req_init.with_headers(Self::knowledge_compiler_headers()?);
        let req = Request::new_with_init(&oembed_url, &req_init)?;
        let mut resp = Fetch::Request(req).send().await?;

        if resp.status_code() != 200 {
            return Ok(None);
        }

        let body: serde_json::Value = resp.json().await?;
        let title = body
            .get("title")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let author = body
            .get("author_name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        Ok(title.map(|t| (t, author)))
    }

    /// Generic fallback for any other web page: fetch the HTML and pull out
    /// <title> and a meta description. No AI involved — this is mechanical
    /// extraction, which is both cheaper and more reliable than asking a
    /// model to guess a page's title from a URL alone.
    ///
    /// This is the authoritative metadata source for links: it runs for every
    /// provider without a dedicated resolver (generic pages, forums, Habr,
    /// arXiv, Wikipedia, ...), and the URL-derived `guess_title` is only used
    /// by the caller if this fetch fails or returns nothing.
    pub async fn resolve_web_title(url: &str) -> Result<Option<(String, Option<String>)>> {
        let mut req_init = RequestInit::new();
        req_init.with_method(Method::Get);
        req_init.with_headers(Self::knowledge_compiler_headers()?);
        let req = Request::new_with_init(url, &req_init)?;
        let mut resp = match Fetch::Request(req).send().await {
            Ok(r) => r,
            Err(e) => {
                crate::log_event!("warn", "resolver.web.fetch_failed", "error={:?}", e);
                return Ok(None);
            }
        };

        if resp.status_code() != 200 {
            crate::log_event!("warn", "resolver.web.bad_status", "url={} status={}", url, resp.status_code());
            return Ok(None);
        }

        let html = resp.text().await?;
        let snippet_len = Self::floor_char_boundary_len(&html, 80_000);
        let snippet = &html[..snippet_len];

        let title = Self::extract_tag_content(snippet, "title")
            .map(|t| Self::decode_html_entities(t.trim()))
            .filter(|t| !t.is_empty())
            .or_else(|| Self::extract_open_graph_title(snippet));
        let description = Self::extract_meta_description(snippet)
            .map(|d| Self::decode_html_entities(d.trim()))
            .filter(|d| !d.is_empty());

        if title.is_none() {
            crate::log_event!("warn", "resolver.web.no_title_found", "url={} html_bytes={}", url, html.len());
        }

        Ok(title.map(|t| (t, description)))
    }

    /// Shared headers for outbound metadata fetches.
    ///
    /// Some public sites (notably DOU) reject unknown bots with 403s even for
    /// ordinary GET requests. A browser-like profile reduces that block rate
    /// without changing the resolver logic itself.
    fn browser_metadata_headers() -> [(&'static str, &'static str); 5] {
        [
            (
                "User-Agent",
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36",
            ),
            (
                "Accept",
                "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8",
            ),
            ("Accept-Language", "ru-RU,ru;q=0.9,en;q=0.8"),
            ("Accept-Encoding", "gzip, deflate, br"),
            ("Cache-Control", "no-cache"),
        ]
    }

    fn knowledge_compiler_headers() -> Result<Headers> {
        let headers = Headers::new();
        for (name, value) in Self::browser_metadata_headers() {
            headers.set(name, value)?;
        }
        Ok(headers)
    }

    /// Largest cut point <= `max` that sits on a UTF-8 char boundary.
    fn floor_char_boundary_len(text: &str, max: usize) -> usize {
        let mut end = max.min(text.len());
        while end > 0 && !text.is_char_boundary(end) {
            end -= 1;
        }
        end
    }

    /// Extracts the text content between an opening and closing tag from HTML,
    /// e.g. `<title>...</title>`. Case-insensitive.
    fn extract_tag_content(html: &str, tag: &str) -> Option<String> {
        let lower = html.to_lowercase();
        let open_tag = format!("<{}", tag);
        let start = lower.find(&open_tag)?;
        let after_open = lower[start..].find('>')? + start + 1;
        let close_tag = format!("</{}>", tag);
        let end_rel = lower[after_open..].find(&close_tag)?;
        Some(html[after_open..after_open + end_rel].to_string())
    }

    /// Extracts the meta description from HTML, preferring `name="description"`
    /// then falling back to `property/name="og:description"`. Decodes entities.
    fn extract_meta_description(html: &str) -> Option<String> {
        Self::extract_meta_value(html, "name", "description")
            .or_else(|| Self::extract_meta_value(html, "property", "og:description"))
            .or_else(|| Self::extract_meta_value(html, "name", "og:description"))
    }

    /// Extracts the Open Graph title from HTML, preferring `property="og:title"`
    /// then `name="og:title"`. Decodes entities.
    fn extract_open_graph_title(html: &str) -> Option<String> {
        Self::extract_meta_value(html, "property", "og:title")
            .or_else(|| Self::extract_meta_value(html, "name", "og:title"))
            .or_else(|| Self::extract_meta_value(html, "property", "twitter:title"))
    }

    /// Find the first `<meta attr="value" ...>` tag (case-insensitive) and return
    /// its `content` attribute. Handles double-quoted, single-quoted and bare
    /// (unquoted) content values, regardless of attribute order.
    fn extract_meta_value(html: &str, attr: &str, value: &str) -> Option<String> {
        let lower = html.to_lowercase();
        let attr_l = attr.to_lowercase();
        let value_l = value.to_lowercase();
        let pair_dq = format!("{}=\"{}\"", attr_l, value_l);
        let pair_sq = format!("{}='{}'", attr_l, value_l);

        let mut search_from = 0usize;
        while let Some(rel) = lower[search_from..].find("<meta") {
            let tag_start = search_from + rel;
            let tag_end_rel = lower[tag_start..].find('>')?;
            let tag = &html[tag_start..tag_start + tag_end_rel];
            let tag_lower = tag.to_lowercase();
            if tag_lower.contains(&pair_dq) || tag_lower.contains(&pair_sq) {
                if let Some(content) = Self::extract_meta_content(tag) {
                    if !content.is_empty() {
                        return Some(content);
                    }
                }
            }
            search_from = tag_start + tag_end_rel;
        }
        None
    }

    /// Extracts the `content` attribute value from a meta tag's opening tag string.
    /// Handles double-quoted, single-quoted, and bare (unquoted) content values.
    fn extract_meta_content(tag: &str) -> Option<String> {
        let tag_lower = tag.to_lowercase();
        let c_pos = tag_lower.find("content")?;
        let rest = tag[c_pos + "content".len()..].trim_start();
        let rest = rest.strip_prefix('=')?.trim_start();
        match rest.chars().next()? {
            '"' => {
                let end = rest[1..].find('"')?;
                Some(rest[1..1 + end].to_string())
            }
            '\'' => {
                let end = rest[1..].find('\'')?;
                Some(rest[1..1 + end].to_string())
            }
            _ => {
                // Bare value: runs until whitespace or '>' (first char is already
                // a non-quote, non-whitespace char after trimming).
                let end = rest
                    .find(|ch: char| ch.is_whitespace() || ch == '>')
                    .unwrap_or(rest.len());
                Some(rest[..end].to_string())
            }
        }
    }

    /// Decodes common HTML entities in a string.
    /// Handles `&amp;`, `&quot;`, `&#39;`, `&apos;`, `&lt;`, `&gt;`, `&nbsp;`,
    /// `&hellip;`, `&copy;`, `&rarr;`, `&rArr;`, and newline/quote (`&#10;`, `&#39;`).
    fn decode_html_entities(s: &str) -> String {
        s.replace("&nbsp;", " ")
            .replace("&hellip;", "…")
            .replace("&copy;", "©")
            .replace("&rarr;", "→")
            .replace("&rArr;", "⇒")
            .replace("&#10;", "\n")
            .replace("&#9;", "\t")
            .replace("&amp;", "&")
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
        assert_eq!(Resolver::parse_github_url("https://example.com"), None);
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
        assert_eq!(
            Resolver::decode_html_entities("Tom &amp; Jerry"),
            "Tom & Jerry"
        );
        assert_eq!(
            Resolver::decode_html_entities("&quot;quoted&quot;"),
            "\"quoted\""
        );
    }

    #[test]
    fn decode_html_entities_should_handle_extra_entities_and_numerics() {
        assert_eq!(Resolver::decode_html_entities("a&nbsp;b"), "a b");
        assert_eq!(Resolver::decode_html_entities("&#39;apos&#39;"), "'apos'");
        assert_eq!(Resolver::decode_html_entities("line&#10;break"), "line\nbreak");
        assert_eq!(Resolver::decode_html_entities("tab&#9;end"), "tab\tend");
        assert_eq!(
            Resolver::decode_html_entities("&hellip;dots&copy;"),
            "…dots©"
        );
    }

#[test]
    fn extract_meta_description_should_handle_single_quoted_content() {
        let html = r#"<meta name='description' content='Сайт про LLM. Зберігає знання'>"#;
        assert_eq!(
            Resolver::extract_meta_description(html),
            Some("Сайт про LLM. Зберігає знання".to_string())
        );
    }

    #[test]
    fn extract_open_graph_title_should_find_og_title() {
        let html = r#"<meta property="og:title" content="Від RAG до LLM Wiki">"#;
        assert_eq!(
            Resolver::extract_open_graph_title(html),
            Some("Від RAG до LLM Wiki".to_string())
        );
    }

    #[test]
    fn floor_char_boundary_len_should_not_split_utf8() {
        // 40_000 Cyrillic 'і' (2 bytes each) == 80_000 bytes, right at the cap;
        // the cut point must still land on a char boundary so a later slice of a
        // large multi-byte page can't panic.
        let text = "і".repeat(40_000);
        let cut = Resolver::floor_char_boundary_len(&text, 80_000);
        assert!(text.is_char_boundary(cut));
        assert_eq!((text[..cut].len() % 2), 0);
    }

    #[test]
    fn knowledge_compiler_headers_should_include_browser_like_headers() {
        let headers = Resolver::browser_metadata_headers();

        let user_agent = headers
            .iter()
            .find(|(name, _)| *name == "User-Agent")
            .map(|(_, value)| *value)
            .unwrap();
        assert!(user_agent.to_lowercase().contains("mozilla"));

        let accept = headers
            .iter()
            .find(|(name, _)| *name == "Accept")
            .map(|(_, value)| *value)
            .unwrap();
        assert!(accept.to_lowercase().contains("text/html"));

        let accept_lang = headers
            .iter()
            .find(|(name, _)| *name == "Accept-Language")
            .map(|(_, value)| *value)
            .unwrap();
        assert!(accept_lang.to_lowercase().contains("ru"));
    }
}
