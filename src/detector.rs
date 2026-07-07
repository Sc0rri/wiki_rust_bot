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

    /// Extract a human-readable title guess from URL path segments.
    /// Handles YouTube (?v=), arXiv (1234.5678), GitHub (owner/repo), etc.
    fn guess_title(url: &str) -> Option<String> {
        let clean = url
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .trim_start_matches("www.");

        // Strip query string and fragment
        let path_only = clean.split(['?', '#']).next().unwrap_or(clean);

        // Split into segments, skip domain (first segment)
        let segments: Vec<&str> = path_only.split('/').skip(1).filter(|s| !s.is_empty()).collect();

        if segments.is_empty() {
            return None;
        }

        // YouTube: youtube.com/watch?v=xxxxx or youtu.be/xxxxx → no useful path
        // arXiv: arxiv.org/abs/1234.5678 → take "1234.5678" (keep dots)
        // GitHub: github.com/owner/repo → "owner/repo"
        // Generic: take last meaningful segment

        let title = match segments.last() {
            Some(last) if last.contains('.') && !last.starts_with("1234") => {
                // Has extension-like dot (e.g. "show", "abs" with dot) — use it but clean
                last.to_string()
            }
            Some(last) => {
                // For arXiv IDs like "1234.5678" or generic slugs
                last.replace('-', " ").replace('_', " ").trim().to_string()
            }
            None => return None,
        };

        // Special handling: GitHub repo (owner/repo)
        if url.contains("github.com") && segments.len() >= 2 {
            return Some(format!("{}/{}", segments[segments.len() - 2], segments.last().unwrap()));
        }

        // Special handling: YouTube watch → no title available
        if url.contains("youtube.com") || url.contains("youtu.be") {
            return None; // Will fall back to "YouTube video" in app
        }

        if title.is_empty() || title.len() > 100 {
            None
        } else {
            Some(title)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_github_url() {
        let r = Detector::detect("https://github.com/tokio-rs/tokio");
        assert_eq!(r.provider, ResourceProvider::Github);
        assert_eq!(r.title.as_deref(), Some("tokio-rs/tokio"));
    }

    #[test]
    fn detect_youtube_url() {
        let r = Detector::detect("https://youtube.com/watch?v=xxxxx");
        assert_eq!(r.provider, ResourceProvider::Youtube);
        assert_eq!(r.title, None);
    }

    #[test]
    fn detect_youtu_be_url() {
        let r = Detector::detect("https://youtu.be/xxxxx");
        assert_eq!(r.provider, ResourceProvider::Youtube);
        assert_eq!(r.title, None);
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
        assert_eq!(r.title.as_deref(), Some("article"));
    }

    #[test]
    fn detect_arxiv_url() {
        let r = Detector::detect("https://arxiv.org/abs/1234.5678");
        assert_eq!(r.provider, ResourceProvider::Arxiv);
        assert_eq!(r.title.as_deref(), Some("1234.5678"));
    }

    #[test]
    fn guess_title_habr() {
        let r = Detector::detect("https://habr.com/ru/articles/123456/");
        assert_eq!(r.title.as_deref(), Some("123456"));
    }
}