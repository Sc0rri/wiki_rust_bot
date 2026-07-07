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

/// What kind of knowledge this represents
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeType {
    Book,
    Movie,
    Series,
    Anime,
    Article,
    Course,
    GithubRepo,
    YoutubeVideo,
    Tool,
    Note,
    Other,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ContentStatus {
    ToRead,
    Read,
    ToWatch,
    Watched,
    Planned,
    InProgress,
    Finished,
    Dropped,
    Using,
    Library,
    Interesting,
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
    pub author: Option<String>,
    pub language: Option<String>,
    pub year: Option<i32>,
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
            status: ContentStatus::ToRead,
            title,
            author: None,
            language: None,
            year: None,
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
    AwaitingRating {
        item: PendingItem,
    },
    AwaitingComment {
        item: PendingItem,
    },
    AwaitingConfirmation {
        item: PendingItem,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum TextTransition {
    Cancel,
    SelectType(KnowledgeType),
    SelectStatus(ContentStatus),
    SetRating(u8),
    SetComment(String),
    Confirm,
    ProcessFresh,
}

impl KnowledgeType {
    pub fn emoji(&self) -> &'static str {
        match self {
            Self::Book => "📚",
            Self::Movie => "🎬",
            Self::Series => "📺",
            Self::Anime => "🎌",
            Self::Article => "📄",
            Self::Course => "🎓",
            Self::GithubRepo => "🐙",
            Self::YoutubeVideo => "▶️",
            Self::Tool => "🛠",
            Self::Note => "📝",
            Self::Other => "📋",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Book => "Book",
            Self::Movie => "Movie",
            Self::Series => "Series",
            Self::Anime => "Anime",
            Self::Article => "Article",
            Self::Course => "Course",
            Self::GithubRepo => "GitHub",
            Self::YoutubeVideo => "YouTube",
            Self::Tool => "Tool",
            Self::Note => "Note",
            Self::Other => "Other",
        }
    }

    pub fn has_status_options(&self) -> bool {
        matches!(self, Self::Book | Self::Movie | Self::Series | Self::Anime | Self::Course | Self::Article | Self::GithubRepo | Self::Tool)
    }
}

impl ContentStatus {
    pub fn label(&self) -> &'static str {
        match self {
            Self::ToRead => "To-read",
            Self::Read => "Read",
            Self::ToWatch => "To-watch",
            Self::Watched => "Watched",
            Self::Planned => "Planned",
            Self::InProgress => "In progress",
            Self::Finished => "Finished",
            Self::Dropped => "Dropped",
            Self::Using => "Using",
            Self::Library => "Library",
            Self::Interesting => "Interesting",
        }
    }

    pub fn is_done(&self) -> bool {
        matches!(self, Self::Read | Self::Watched | Self::Finished | Self::Dropped)
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

impl DetectedResource {
    pub fn preview_text(&self) -> String {
        let provider = self.provider.label();
        let title = self.title.as_deref().unwrap_or("Untitled");
        format!("🔗 {}: {}", provider, title)
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
                } else if lower.contains("article") || lower.contains("статья") {
                    TextTransition::SelectType(KnowledgeType::Article)
                } else if lower.contains("course") || lower.contains("курс") {
                    TextTransition::SelectType(KnowledgeType::Course)
                } else if lower.contains("github") || lower.contains("репозиторий") {
                    TextTransition::SelectType(KnowledgeType::GithubRepo)
                } else if lower.contains("youtube") || lower.contains("видео") || lower.contains("ютуб") {
                    TextTransition::SelectType(KnowledgeType::YoutubeVideo)
                } else if lower.contains("tool") || lower.contains("инструмент") {
                    TextTransition::SelectType(KnowledgeType::Tool)
                } else if lower.contains("note") || lower.contains("заметк") || lower.contains("idea") || lower.contains("идея") {
                    TextTransition::SelectType(KnowledgeType::Note)
                } else {
                    TextTransition::SelectType(KnowledgeType::Other)
                }
            }
            Self::AwaitingStatus { .. } => {
                if lower.contains("to-read") || lower.contains("to-watch") || lower.contains("planned") || lower.contains("отложен") {
                    TextTransition::SelectStatus(ContentStatus::ToRead)
                } else if lower.contains("read") || lower.contains("watched") || lower.contains("finished") || lower.contains("прочитан") || lower.contains("посмотрел") {
                    TextTransition::SelectStatus(ContentStatus::Read)
                } else if lower.contains("dropped") || lower.contains("бросил") {
                    TextTransition::SelectStatus(ContentStatus::Dropped)
                } else if lower.contains("using") || lower.contains("использую") {
                    TextTransition::SelectStatus(ContentStatus::Using)
                } else if lower.contains("library") || lower.contains("библиотек") {
                    TextTransition::SelectStatus(ContentStatus::Library)
                } else if lower.contains("interesting") || lower.contains("интересн") {
                    TextTransition::SelectStatus(ContentStatus::Interesting)
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
                if lower == "skip" || lower == "пропустить" || lower == "далее" {
                    TextTransition::Confirm
                } else {
                    TextTransition::ProcessFresh
                }
            }
            Self::AwaitingComment { .. } => {
                if lower == "skip" || lower == "пропустить" || lower == "далее" {
                    TextTransition::Confirm
                } else {
                    TextTransition::SetComment(text.to_string())
                }
            }
            Self::AwaitingConfirmation { .. } => {
                if lower == "confirm" || lower == "✅ save" || lower == "да" || lower == "сохранить" {
                    TextTransition::Confirm
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
        assert_eq!(KnowledgeType::GithubRepo.emoji(), "🐙");
        assert_eq!(KnowledgeType::YoutubeVideo.emoji(), "▶️");
    }

    #[test]
    fn content_status_label_should_return_correct_label() {
        assert_eq!(ContentStatus::ToRead.label(), "To-read");
        assert_eq!(ContentStatus::Watched.label(), "Watched");
        assert_eq!(ContentStatus::InProgress.label(), "In progress");
    }

    #[test]
    fn pending_item_should_generate_id() {
        let item = PendingItem::new("Test Title".to_string(), KnowledgeType::Book);
        assert!(!item.id.is_empty());
        assert_eq!(item.source, "telegram");
        assert!(!item.processed);
    }
}