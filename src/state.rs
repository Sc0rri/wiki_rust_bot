use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct PendingItem {
    pub title: String,
    pub content_type: ContentType,
    pub status: ContentStatus,
    pub category: Option<String>,
    pub url: Option<String>,
    pub author: Option<String>,
    pub year: Option<i32>,
    pub description: Option<String>,
    pub tags: Vec<String>,
    pub source: String,
    pub processed: bool,
}

impl PendingItem {
    pub fn new(title: String, content_type: ContentType) -> Self {
        Self {
            title,
            content_type,
            status: ContentStatus::ToRead,
            category: None,
            url: None,
            author: None,
            year: None,
            description: None,
            tags: Vec::new(),
            source: String::new(),
            processed: false,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ContentType {
    Book,
    Movie,
    Series,
    Anime,
    Article,
    Course,
    Paper,
    Tool,
    Pdf,
    Image,
    Idea,
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

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "type", content = "data")]
pub enum UserState {
    None,
    AwaitingType { raw_text: String },
    AwaitingCategory { title: String, content_type: ContentType },
    AwaitingStatus { item: PendingItem },
    AwaitingDetails { item: PendingItem },
    AwaitingConfirmation { item: PendingItem },
}

#[derive(Debug, Clone, PartialEq)]
pub enum TextTransition {
    Cancel,
    SelectType(ContentType),
    SelectCategory(String),
    SelectStatus(ContentStatus),
    UpdateDetails { field: String, value: String },
    Confirm,
    ProcessFresh,
}

impl ContentType {
    pub fn emoji(&self) -> &'static str {
        match self {
            Self::Book => "📚",
            Self::Movie => "🎬",
            Self::Series => "📺",
            Self::Anime => "🎌",
            Self::Article => "📄",
            Self::Course => "🎓",
            Self::Paper => "📑",
            Self::Tool => "🛠",
            Self::Pdf => "📕",
            Self::Image => "🖼",
            Self::Idea => "💡",
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
            Self::Paper => "Paper",
            Self::Tool => "Tool",
            Self::Pdf => "PDF",
            Self::Image => "Image",
            Self::Idea => "Idea",
            Self::Note => "Note",
            Self::Other => "Other",
        }
    }

    pub fn has_categories(&self) -> bool {
        matches!(self, Self::Article | Self::Pdf | Self::Image)
    }

    pub fn has_status_options(&self) -> bool {
        matches!(self, Self::Book | Self::Movie | Self::Series | Self::Anime | Self::Course | Self::Tool)
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
                    TextTransition::SelectType(ContentType::Book)
                } else if lower.contains("movie") || lower.contains("фильм") {
                    TextTransition::SelectType(ContentType::Movie)
                } else if lower.contains("series") || lower.contains("сериал") {
                    TextTransition::SelectType(ContentType::Series)
                } else if lower.contains("anime") || lower.contains("аним") {
                    TextTransition::SelectType(ContentType::Anime)
                } else if lower.contains("article") || lower.contains("статья") {
                    TextTransition::SelectType(ContentType::Article)
                } else if lower.contains("course") || lower.contains("курс") {
                    TextTransition::SelectType(ContentType::Course)
                } else if lower.contains("paper") {
                    TextTransition::SelectType(ContentType::Paper)
                } else if lower.contains("tool") || lower.contains("инструмент") {
                    TextTransition::SelectType(ContentType::Tool)
                } else if lower.contains("pdf") {
                    TextTransition::SelectType(ContentType::Pdf)
                } else if lower.contains("image") || lower.contains("изображен") {
                    TextTransition::SelectType(ContentType::Image)
                } else if lower.contains("idea") || lower.contains("идея") {
                    TextTransition::SelectType(ContentType::Idea)
                } else if lower.contains("note") || lower.contains("заметк") {
                    TextTransition::SelectType(ContentType::Note)
                } else {
                    TextTransition::SelectType(ContentType::Other)
                }
            }
            Self::AwaitingCategory { .. } => {
                TextTransition::SelectCategory(text.to_string())
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
            Self::AwaitingDetails { .. } => {
                if lower == "skip" || lower == "пропустить" || lower == "далее" {
                    TextTransition::Confirm
                } else if lower.starts_with("author:") || lower.starts_with("автор:") {
                    TextTransition::UpdateDetails {
                        field: "author".to_string(),
                        value: text.trim().to_string(),
                    }
                } else if lower.starts_with("year:") || lower.starts_with("год:") {
                    TextTransition::UpdateDetails {
                        field: "year".to_string(),
                        value: text.trim().to_string(),
                    }
                } else if lower.starts_with("tag:") || lower.starts_with("тег:") {
                    TextTransition::UpdateDetails {
                        field: "tag".to_string(),
                        value: text.trim().to_string(),
                    }
                } else {
                    TextTransition::ProcessFresh
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
    fn content_type_emoji_should_return_correct_emoji() {
        assert_eq!(ContentType::Book.emoji(), "📚");
        assert_eq!(ContentType::Article.emoji(), "📄");
        assert_eq!(ContentType::Course.emoji(), "🎓");
    }

    #[test]
    fn content_status_label_should_return_correct_label() {
        assert_eq!(ContentStatus::ToRead.label(), "To-read");
        assert_eq!(ContentStatus::Watched.label(), "Watched");
        assert_eq!(ContentStatus::InProgress.label(), "In progress");
    }

    #[test]
    fn text_transition_should_select_article_type() {
        let state = UserState::AwaitingType {
            raw_text: "Docker networking".to_string(),
        };
        assert_eq!(
            state.text_transition("article"),
            TextTransition::SelectType(ContentType::Article)
        );
    }
}