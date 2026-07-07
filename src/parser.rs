use crate::state::{ContentStatus, ContentType};

pub struct ParserService;

impl ParserService {
    pub fn is_url(text: &str) -> bool {
        text.starts_with("http://") || text.starts_with("https://")
    }

    pub fn detect_content_type_from_url(url: &str) -> Option<ContentType> {
        let lower = url.to_lowercase();
        
        if lower.contains("amazon") && (lower.contains("book") || lower.contains("dp/")) {
            Some(ContentType::Book)
        } else if lower.contains("goodreads") || lower.contains("litres") || lower.contains("books") {
            Some(ContentType::Book)
        } else if lower.contains("imdb.com/title") || lower.contains("kinopoisk") {
            Some(ContentType::Movie)
        } else if lower.contains("myanimelist") || lower.contains("anidb") || lower.contains("shikimori") {
            Some(ContentType::Anime)
        } else if lower.contains("youtube.com") || lower.contains("youtu.be") {
            Some(ContentType::Movie) // Default to Movie, user can change
        } else if lower.contains("github.com") {
            Some(ContentType::Tool)
        } else if lower.contains("coursera.org") || lower.contains("udemy.com") {
            Some(ContentType::Course)
        } else if lower.contains("arxiv.org") {
            Some(ContentType::Paper)
        } else if lower.contains("pdf") || lower.ends_with(".pdf") {
            Some(ContentType::Pdf)
        } else {
            None
        }
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

    pub fn generate_filename(title: &str, content_type: &ContentType, status: &ContentStatus) -> String {
        let now = chrono::Utc::now().format("%Y-%m-%d");
        let slug = Self::slugify(title);
        
        format!("{}/{}/{}_{}.md", content_type.label().to_lowercase(), status.label().to_lowercase(), now, slug)
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
    fn detect_content_type_should_identify_book_urls() {
        assert_eq!(
            ParserService::detect_content_type_from_url("https://www.amazon.com/dp/123456"),
            Some(ContentType::Book)
        );
        assert_eq!(
            ParserService::detect_content_type_from_url("https://www.goodreads.com/book/show/123"),
            Some(ContentType::Book)
        );
    }

    #[test]
    fn detect_content_type_should_identify_anime_urls() {
        assert_eq!(
            ParserService::detect_content_type_from_url("https://myanimelist.net/anime/123"),
            Some(ContentType::Anime)
        );
    }

    #[test]
    fn detect_content_type_should_identify_paper_urls() {
        assert_eq!(
            ParserService::detect_content_type_from_url("https://arxiv.org/abs/1234.5678"),
            Some(ContentType::Paper)
        );
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
            &ContentType::Book,
            &ContentStatus::ToRead
        );
        assert!(result.starts_with("book/to-read/"));
        assert!(result.ends_with(".md"));
    }
}