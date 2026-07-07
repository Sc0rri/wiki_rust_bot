use crate::state::KnowledgeType;

pub struct ParserService;

impl ParserService {
    pub fn is_url(text: &str) -> bool {
        text.starts_with("http://") || text.starts_with("https://")
    }

    pub fn slugify(text: &str) -> String {
        text.to_lowercase()
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '-' })
            .collect::<String>()
            .split('-')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("-")
    }

    pub fn generate_filename(title: &str, knowledge_type: &KnowledgeType, status: &str) -> String {
        let now = chrono::Utc::now().format("%Y-%m-%d");
        let slug = Self::slugify(title);
        format!("{}/{}/{}_{}.md", knowledge_type.label().to_lowercase(), status, now, slug)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn generate_filename_should_create_correct_path() {
        let result = ParserService::generate_filename(
            "Lord of the Rings",
            &KnowledgeType::Book,
            "to-read"
        );
        assert!(result.starts_with("book/to-read/"));
        assert!(result.ends_with(".md"));
    }
}