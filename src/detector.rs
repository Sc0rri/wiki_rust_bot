use crate::state::{DetectedResource, ResourceProvider, ResourceType};

/// Detects resource metadata from a URL. No business logic — pure detection.
pub struct Detector;

impl Detector {
    pub fn detect(url: &str) -> DetectedResource {
        let lower = url.to_lowercase();
        
        let provider = if lower.contains("github.com") {
            ResourceProvider::Github
        } else if lower.contains("youtube.com") || lower.contains("youtu.be") {
            ResourceProvider::Youtube
        } else if lower.contains("goodreads.com") {
            ResourceProvider::Goodreads
        } else if lower.contains("imdb.com") || lower.contains("kinopoisk.ru") {
            ResourceProvider::Imdb
        } else if lower.contains("arxiv.org") {
            ResourceProvider::Arxiv
        } else if lower.contains("coursera.org") || lower.contains("udemy.com") || lower.contains("stepik.org") {
            ResourceProvider::Coursera
        } else if lower.contains("habr.com") {
            ResourceProvider::Habr
        } else if lower.contains("wikipedia.org") {
            ResourceProvider::Wikipedia
        } else {
            ResourceProvider::Web
        };

        let title = Self::guess_title(url);

        DetectedResource {
            provider,
            resource_type: ResourceType::Url,
            url: url.to_string(),
            title,
            description: None,
        }
    }

    /// Simple title extraction from URL path (just a guess, not AI)
    fn guess_title(url: &str) -> Option<String> {
        let clean = url
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .trim_start_matches("www.");
        
        if let Some(path) = clean.split('/').skip(1).collect::<Vec<_>>().join("/").split('?').next() {
            let title = path
                .split('/')
                .filter(|s| !s.is_empty() && s.len() > 2 && !s.contains('.'))
                .last()
                .unwrap_or(path)
                .replace('-', " ")
                .replace('_', " ")
                .trim()
                .to_string();
            
            if !title.is_empty() && title.len() < 100 {
                return Some(title);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_github_url() {
        let r = Detector::detect("https://github.com/tokio-rs/tokio");
        assert_eq!(r.provider, ResourceProvider::Github);
    }

    #[test]
    fn detect_youtube_url() {
        let r = Detector::detect("https://youtube.com/watch?v=xxxxx");
        assert_eq!(r.provider, ResourceProvider::Youtube);
    }

    #[test]
    fn detect_youtu_be_url() {
        let r = Detector::detect("https://youtu.be/xxxxx");
        assert_eq!(r.provider, ResourceProvider::Youtube);
    }

    #[test]
    fn detect_goodreads_url() {
        let r = Detector::detect("https://www.goodreads.com/book/show/123");
        assert_eq!(r.provider, ResourceProvider::Goodreads);
    }

    #[test]
    fn detect_web_url() {
        let r = Detector::detect("https://example.com/article");
        assert_eq!(r.provider, ResourceProvider::Web);
    }

    #[test]
    fn detect_arxiv_url() {
        let r = Detector::detect("https://arxiv.org/abs/1234.5678");
        assert_eq!(r.provider, ResourceProvider::Arxiv);
    }
}