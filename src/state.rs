use serde::{Deserialize, Serialize};

/// How the resource was provided (input method)
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ResourceType {
    Url,
    Text,
    Pdf,
    Image,
}

/// Provider/source of the resource (when applicable)
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ResourceProvider {
    Github,
    Youtube,
    Goodreads,
    Imdb,
    Arxiv,
    Coursera,
    Habr,
    Wikipedia,
    Web,
    Direct,
}

/// What kind of knowledge this represents.
///
/// Only Book/Movie/Series/Anime get the full status+rating+comment flow —
/// those are the only types where "did I finish it, was it good" is a
/// meaningful question. Everything else that arrives as a URL is just a
/// `Link` (optional comment only); everything else that arrives as plain
/// text/media and isn't clearly one of the four is a `Note`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeType {
    Book,
    Movie,
    Series,
    Anime,
    Link,
    Note,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ContentStatus {
    Backlog,
    Done,
    Dropped,
}

/// Detected resource from URL analysis (no business logic)
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct DetectedResource {
    pub provider: ResourceProvider,
    pub resource_type: ResourceType,
    pub url: String,
    pub title: Option<String>,
    pub description: Option<String>,
}

/// Full pending item with rich metadata
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct PendingItem {
    pub id: String,
    pub created: String,
    pub source: String,
    pub provider: ResourceProvider,
    pub url: Option<String>,
    pub knowledge_type: KnowledgeType,
    pub status: ContentStatus,
    pub title: String,
    /// The original, unprocessed text this item came from — the raw message
    /// text (before AI picked a title), or a photo/PDF caption. Kept
    /// separate from `title` (which can be AI-derived or a generic
    /// placeholder for links) and `comment` (a follow-up the user adds
    /// interactively), so the source material survives even if the derived
    /// title turns out wrong or generic — useful for reprocessing later
    /// with a better model, without needing to go back to Telegram.
    pub raw_text: Option<String>,
    pub author: Option<String>,
    pub language: Option<String>,
    pub year: Option<i32>,
    pub season: Option<u32>,
    pub stars: Option<i32>,
    pub rating: Option<u8>,
    pub comment: Option<String>,
    pub description: Option<String>,
    pub tags: Vec<String>,
    pub processed: bool,
}

impl PendingItem {
    pub fn new(title: String, knowledge_type: KnowledgeType) -> Self {
        let now = chrono::Utc::now();
        Self {
            id: format!("{}-{}", now.format("%Y%m%d%H%M%S"), title.chars().take(20).collect::<String>().to_lowercase().replace(' ', "-")),
            created: now.format("%Y-%m-%d").to_string(),
            source: "telegram".to_string(),
            provider: ResourceProvider::Direct,
            url: None,
            knowledge_type,
            status: ContentStatus::Backlog,
            title,
            raw_text: None,
            author: None,
            language: None,
            year: None,
            season: None,
            stars: None,
            rating: None,
            comment: None,
            description: None,
            tags: Vec::new(),
            processed: false,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "type", content = "data")]
pub enum UserState {
    None,
    AwaitingType {
        raw_text: String,
        detected: Option<DetectedResource>,
        media_file_id: Option<String>,
    },
    AwaitingStatus {
        item: PendingItem,
    },
    AwaitingSeason {
        item: PendingItem,
    },
    AwaitingRating {
        item: PendingItem,
    },
    AwaitingComment {
        item: PendingItem,
    },
    AwaitingAiConfirm {
        item: PendingItem,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum TextTransition {
    Cancel,
    SelectType(KnowledgeType),
    SelectStatus(ContentStatus),
    SetSeason(Option<u32>),
    SetRating(u8),
    SetComment(String),
    ConfirmAi,
    ProcessFresh,
}

impl KnowledgeType {
    pub fn emoji(&self) -> &'static str {
        match self {
            Self::Book => "📚",
            Self::Movie => "🎬",
            Self::Series => "📺",
            Self::Anime => "🎌",
            Self::Link => "🔗",
            Self::Note => "📝",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Book => "Book",
            Self::Movie => "Movie",
            Self::Series => "Series",
            Self::Anime => "Anime",
            Self::Link => "Link",
            Self::Note => "Note",
        }
    }

    /// Only media types get status/rating — a Link or Note has nothing
    /// meaningful to track beyond an optional comment.
    pub fn has_status_options(&self) -> bool {
        matches!(self, Self::Book | Self::Movie | Self::Series | Self::Anime)
    }
}

impl ContentStatus {
    pub fn label(&self, kt: &KnowledgeType) -> &'static str {
        match self {
            Self::Backlog => match kt {
                KnowledgeType::Book => "To-read",
                KnowledgeType::Movie | KnowledgeType::Series | KnowledgeType::Anime => "To-watch",
                _ => "Backlog",
            },
            Self::Done => match kt {
                KnowledgeType::Book => "Read",
                KnowledgeType::Movie | KnowledgeType::Series | KnowledgeType::Anime => "Watched",
                _ => "Done",
            },
            Self::Dropped => "Dropped",
        }
    }

    /// Whether this status should prompt for a rating (only for completed/dropped items)
    pub fn needs_rating(&self) -> bool {
        matches!(self, Self::Done | Self::Dropped)
    }
}

impl ResourceProvider {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Github => "GitHub",
            Self::Youtube => "YouTube",
            Self::Goodreads => "Goodreads",
            Self::Imdb => "IMDb",
            Self::Arxiv => "arXiv",
            Self::Coursera => "Coursera",
            Self::Habr => "Habr",
            Self::Wikipedia => "Wikipedia",
            Self::Web => "Web",
            Self::Direct => "",
        }
    }
}

impl UserState {
    pub fn parse_or_none(raw: &str) -> Self {
        serde_json::from_str(raw).unwrap_or(Self::None)
    }

    pub fn text_transition(&self, text: &str) -> TextTransition {
        let lower = text.to_lowercase();

        if lower == "cancel" || lower == "❌ cancel" {
            return TextTransition::Cancel;
        }

        match self {
            Self::AwaitingType { .. } => {
                if lower.contains("book") || lower.contains("книг") {
                    TextTransition::SelectType(KnowledgeType::Book)
                } else if lower.contains("movie") || lower.contains("фильм") {
                    TextTransition::SelectType(KnowledgeType::Movie)
                } else if lower.contains("series") || lower.contains("сериал") {
                    TextTransition::SelectType(KnowledgeType::Series)
                } else if lower.contains("anime") || lower.contains("аним") {
                    TextTransition::SelectType(KnowledgeType::Anime)
                } else {
                    TextTransition::SelectType(KnowledgeType::Note)
                }
            }
            Self::AwaitingStatus { .. } => {
                if lower.contains("backlog") || lower.contains("to-read") || lower.contains("to-watch") || lower.contains("отложен") {
                    TextTransition::SelectStatus(ContentStatus::Backlog)
                } else if lower.contains("done") || lower.contains("read") || lower.contains("watched") || lower.contains("прочитан") || lower.contains("посмотрел") {
                    TextTransition::SelectStatus(ContentStatus::Done)
                } else if lower.contains("dropped") || lower.contains("бросил") {
                    TextTransition::SelectStatus(ContentStatus::Dropped)
                } else {
                    TextTransition::ProcessFresh
                }
            }
            Self::AwaitingSeason { .. } => {
                if let Ok(season) = lower.parse::<u32>() {
                    if season >= 1 {
                        return TextTransition::SetSeason(Some(season));
                    }
                }
                if lower.contains("skip") || lower.contains("пропустить") || lower == "далее" {
                    TextTransition::SetSeason(None)
                } else {
                    TextTransition::ProcessFresh
                }
            }
            Self::AwaitingRating { .. } => {
                if let Ok(rating) = lower.parse::<u8>() {
                    if rating >= 1 && rating <= 10 {
                        return TextTransition::SetRating(rating);
                    }
                }
                if lower.contains("skip") || lower.contains("пропустить") || lower == "далее" {
                    TextTransition::SetRating(0) // 0 = skipped
                } else {
                    TextTransition::ProcessFresh
                }
            }
            Self::AwaitingComment { .. } => {
                if lower.contains("skip") || lower.contains("пропустить") || lower == "далее" {
                    TextTransition::SetComment(String::new())
                } else {
                    TextTransition::SetComment(text.to_string())
                }
            }
            Self::AwaitingAiConfirm { .. } => {
                if lower == "confirm" || lower == "✅ confirm" || lower == "да" || lower == "подтвердить" {
                    TextTransition::ConfirmAi
                } else if lower.contains("book") || lower.contains("книг") {
                    TextTransition::SelectType(KnowledgeType::Book)
                } else if lower.contains("movie") || lower.contains("фильм") {
                    TextTransition::SelectType(KnowledgeType::Movie)
                } else if lower.contains("series") || lower.contains("сериал") {
                    TextTransition::SelectType(KnowledgeType::Series)
                } else if lower.contains("anime") || lower.contains("аним") {
                    TextTransition::SelectType(KnowledgeType::Anime)
                } else if lower.contains("note") || lower.contains("заметк") {
                    TextTransition::SelectType(KnowledgeType::Note)
                } else {
                    TextTransition::ProcessFresh
                }
            }
            Self::None => TextTransition::ProcessFresh,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knowledge_type_emoji_should_return_correct_emoji() {
        assert_eq!(KnowledgeType::Book.emoji(), "📚");
        assert_eq!(KnowledgeType::Link.emoji(), "🔗");
    }

    #[test]
    fn content_status_label_should_return_correct_label() {
        let book = KnowledgeType::Book;
        let movie = KnowledgeType::Movie;
        let note = KnowledgeType::Note;
        assert_eq!(ContentStatus::Backlog.label(&book), "To-read");
        assert_eq!(ContentStatus::Backlog.label(&movie), "To-watch");
        assert_eq!(ContentStatus::Backlog.label(&note), "Backlog");
        assert_eq!(ContentStatus::Done.label(&book), "Read");
        assert_eq!(ContentStatus::Done.label(&movie), "Watched");
        assert_eq!(ContentStatus::Dropped.label(&book), "Dropped");
    }

    #[test]
    fn has_status_options_should_be_true_only_for_media_types() {
        assert!(KnowledgeType::Book.has_status_options());
        assert!(KnowledgeType::Movie.has_status_options());
        assert!(KnowledgeType::Series.has_status_options());
        assert!(KnowledgeType::Anime.has_status_options());
        assert!(!KnowledgeType::Link.has_status_options());
        assert!(!KnowledgeType::Note.has_status_options());
    }

    #[test]
    fn pending_item_should_generate_id() {
        let item = PendingItem::new("Test Title".to_string(), KnowledgeType::Book);
        assert!(!item.id.is_empty());
        assert_eq!(item.source, "telegram");
        assert!(!item.processed);
        assert_eq!(item.status, ContentStatus::Backlog);
    }
}
