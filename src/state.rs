use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct PendingItem {
    pub title: String,
    pub content_type: ContentType,
    pub status: ContentStatus,
    pub url: Option<String>,
    pub author: Option<String>,
    pub year: Option<i32>,
    pub description: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ContentType {
    Book,
    Movie,
    Series,
    Anime,
    Other,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ContentStatus {
    Done,
    Pending,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "type", content = "data")]
pub enum UserState {
    None,
    AwaitingType { raw_text: String },
    AwaitingStatus { title: String, content_type: ContentType },
    AwaitingDetails { item: PendingItem },
    AwaitingConfirmation { item: PendingItem },
}

#[derive(Debug, Clone, PartialEq)]
pub enum TextTransition {
    Cancel,
    SelectType(ContentType),
    SelectStatus(ContentStatus),
    UpdateDetails { field: String, value: String },
    Confirm,
    ProcessFresh,
}

impl ContentType {
    pub fn folder(&self) -> &'static str {
        match self {
            Self::Book => "books",
            Self::Movie => "movies",
            Self::Series => "series",
            Self::Anime => "anime",
            Self::Other => "watchlist",
        }
    }

    pub fn emoji(&self) -> &'static str {
        match self {
            Self::Book => "📚",
            Self::Movie => "🎬",
            Self::Series => "📺",
            Self::Anime => "🎌",
            Self::Other => "📋",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Book => "Book",
            Self::Movie => "Movie",
            Self::Series => "Series",
            Self::Anime => "Anime",
            Self::Other => "Other",
        }
    }
}

impl ContentStatus {
    pub fn folder(&self) -> &'static str {
        match self {
            Self::Done => "read",
            Self::Pending => "to-read",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Done => "Done",
            Self::Pending => "To-read",
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
                    TextTransition::SelectType(ContentType::Book)
                } else if lower.contains("movie") || lower.contains("фильм") {
                    TextTransition::SelectType(ContentType::Movie)
                } else if lower.contains("series") || lower.contains("сериал") {
                    TextTransition::SelectType(ContentType::Series)
                } else if lower.contains("anime") || lower.contains("аним") {
                    TextTransition::SelectType(ContentType::Anime)
                } else {
                    TextTransition::ProcessFresh
                }
            }
            Self::AwaitingStatus { .. } => {
                if lower.contains("done") || lower.contains("read") || lower.contains("watched") || lower.contains("прочитан") || lower.contains("посмотрел") {
                    TextTransition::SelectStatus(ContentStatus::Done)
                } else if lower.contains("to-read") || lower.contains("to-watch") || lower.contains("список") || lower.contains("отложен") {
                    TextTransition::SelectStatus(ContentStatus::Pending)
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
    fn content_type_folder_should_return_correct_path() {
        assert_eq!(ContentType::Book.folder(), "books");
        assert_eq!(ContentType::Movie.folder(), "movies");
        assert_eq!(ContentType::Series.folder(), "series");
        assert_eq!(ContentType::Anime.folder(), "anime");
    }

    #[test]
    fn content_status_folder_should_return_correct_path() {
        assert_eq!(ContentStatus::Done.folder(), "read");
        assert_eq!(ContentStatus::Pending.folder(), "to-read");
    }

    #[test]
    fn text_transition_should_select_book_type() {
        let state = UserState::AwaitingType {
            raw_text: "Lord of the Rings".to_string(),
        };
        assert_eq!(
            state.text_transition("book"),
            TextTransition::SelectType(ContentType::Book)
        );
    }

    #[test]
    fn text_transition_should_select_done_status() {
        let state = UserState::AwaitingStatus {
            title: "Lord of the Rings".to_string(),
            content_type: ContentType::Book,
        };
        assert_eq!(
            state.text_transition("done"),
            TextTransition::SelectStatus(ContentStatus::Done)
        );
    }
}