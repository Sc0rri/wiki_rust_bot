use crate::state::PendingItem;

pub struct ParserService;

impl ParserService {
    pub fn is_url(text: &str) -> bool {
        text.starts_with("http://") || text.starts_with("https://")
    }

    /// Slugifies text for use in a filename/URL path. Length is capped
    /// because titles can be an entire forwarded paragraph (e.g. a long
    /// Note) — an unbounded slug turns into a GitHub API request URL long
    /// enough that GitHub rejects it outright ("Request-URL too long").
    pub fn slugify(text: &str) -> String {
        const MAX_SLUG_CHARS: usize = 60;
        let slug = text
            .to_lowercase()
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '-' })
            .collect::<String>()
            .split('-')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("-");

        if slug.chars().count() > MAX_SLUG_CHARS {
            slug.chars().take(MAX_SLUG_CHARS).collect()
        } else {
            slug
        }
    }

    pub fn generate_filename(item: &PendingItem) -> String {
        let now = chrono::Utc::now().format("%Y-%m-%d_%H%M");
        let slug = Self::slugify(&item.title);
        format!("{}_{}.yaml", now, slug)
    }

    /// Same naming convention as generate_filename, so an asset and its
    /// pending YAML entry are easy to correlate by eye in inbox/.
    pub fn generate_asset_filename(item: &PendingItem, extension: &str) -> String {
        let now = chrono::Utc::now().format("%Y-%m-%d_%H%M");
        let slug = Self::slugify(&item.title);
        format!("{}_{}.{}", now, slug, extension)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{KnowledgeType, PendingItem};

    #[test]
    fn is_url_should_detect_https() {
        assert!(ParserService::is_url("https://example.com"));
    }

    #[test]
    fn is_url_should_detect_http() {
        assert!(ParserService::is_url("http://example.com"));
    }

    #[test]
    fn is_url_should_reject_plain_text() {
        assert!(!ParserService::is_url("Lord of the Rings"));
    }

    #[test]
    fn slugify_should_create_url_slug() {
        assert_eq!(ParserService::slugify("Lord of the Rings"), "lord-of-the-rings");
        assert_eq!(ParserService::slugify("The Matrix (1999)"), "the-matrix-1999");
    }

    #[test]
    fn slugify_should_cap_length_for_long_text() {
        let long_text = "a ".repeat(200); // 400 chars of "a a a a ..."
        let slug = ParserService::slugify(&long_text);
        assert!(slug.chars().count() <= 60, "slug was {} chars", slug.chars().count());
    }

    #[test]
    fn generate_filename_should_create_flat_yaml() {
        let item = PendingItem::new("Lord of the Rings".to_string(), KnowledgeType::Book);
        let result = ParserService::generate_filename(&item);
        assert!(result.ends_with(".yaml"));
        assert!(!result.contains('/'));
        assert!(result.contains("lord-of-the-rings"));
    }
}