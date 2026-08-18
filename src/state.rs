use serde::{Deserialize, Serialize};

/// How the resource was provided (input method).
/// - `Url`: submitted as a link
/// - `Text`: plain text content
/// - `Pdf`: a PDF document
/// - `Image`: an image/photo
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

/// The status of a media item in the reading/watching tracking flow.
/// - `Backlog`: To-read / To-watch (queued)
/// - `Done`: finished it (Read / Watched)
/// - `Dropped`: abandoned
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
    /// The Telegram chat_id this item was submitted from — needed so the
    /// external clarification script knows where to send follow-up questions.
    pub chat_id: i64,
    /// The original, unprocessed text this item came from — the raw message
    /// text, or a photo/PDF caption. Kept separate from `title` (which can
    /// be a generic placeholder for links) and `comment` (a follow-up the
    /// user adds interactively), so the source material survives even if
    /// the derived title turns out wrong or generic.
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
    pub asset_sha256: Option<String>,
    pub asset_mime: Option<String>,
    pub asset_width: Option<i64>,
    pub asset_height: Option<i64>,
}

/// Removes invisible Unicode characters (zero-width spaces, BOM) from a string.
/// These can be accidentally copied from formatted text and cause issues in YAML.
fn strip_invisible_chars(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, '\u{200B}' | '\u{200C}' | '\u{200D}' | '\u{FEFF}'))
        .collect()
}

/// Replaces characters that are illegal in a Git/GitHub or local filesystem
/// path component with `-`. Keeps Unicode letters (including Cyrillic) so
/// non-ASCII titles still produce readable ids, but guarantees the result can
/// never contain a path separator (`/` or `\`) — a GitHub `owner/repo` title
/// would otherwise bake a `/` into the item id and commit the file into a
/// nested `inbox/pending/<owner>/` directory instead of a flat path.
pub(crate) fn sanitize_path_component(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c == ' '
                || c == '/'
                || c == '\\'
                || c == ':'
                || c == '*'
                || c == '?'
                || c == '"'
                || c == '<'
                || c == '>'
                || c == '|'
                || c.is_control()
            {
                '-'
            } else {
                c
            }
        })
        .collect()
}

impl PendingItem {
    /// Creates a new `PendingItem` with the given title, knowledge type, and chat_id.
    /// Strips invisible characters from the title and generates a unique ID from
    /// the current timestamp plus the first 20 chars of the (lowercased) title.
    /// Defaults to `Backlog` status, empty tags, and `telegram` source.
    pub fn new(title: String, knowledge_type: KnowledgeType, chat_id: i64) -> Self {
        let title = strip_invisible_chars(&title);
        let now = chrono::Utc::now();
        // The id is used verbatim as a file path component (pending YAML,
        // archive asset, reply file), so every path/illegal character from the
        // title must be flattened — a `/` in a GitHub `owner/repo` title would
        // otherwise commit the item into a nested inbox/pending/<owner>/
        // directory instead of a flat file.
        let id_fragment =
            sanitize_path_component(&title.chars().take(20).collect::<String>().to_lowercase());
        Self {
            id: format!("{}-{}", now.format("%Y%m%d%H%M%S"), id_fragment),
            created: now.format("%Y-%m-%d").to_string(),
            source: "telegram".to_string(),
            provider: ResourceProvider::Direct,
            url: None,
            knowledge_type,
            status: ContentStatus::Backlog,
            title,
            chat_id,
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
            asset_sha256: None,
            asset_mime: None,
            asset_width: None,
            asset_height: None,
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
}

#[derive(Debug, Clone, PartialEq)]
pub enum TextTransition {
    Cancel,
    SelectType(KnowledgeType),
    SelectStatus(ContentStatus),
    SetSeason(Option<u32>),
    SetRating(u8),
    SetComment(String),
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
                if lower.contains("backlog")
                    || lower.contains("to-read")
                    || lower.contains("to-watch")
                    || lower.contains("отложен")
                {
                    TextTransition::SelectStatus(ContentStatus::Backlog)
                } else if lower.contains("done")
                    || lower.contains("read")
                    || lower.contains("watched")
                    || lower.contains("прочитан")
                    || lower.contains("посмотрел")
                {
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
                if lower.contains("skip") || lower.contains("пропустить") || lower == "далее"
                {
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
                if lower.contains("skip") || lower.contains("пропустить") || lower == "далее"
                {
                    TextTransition::SetRating(0) // 0 = skipped
                } else {
                    TextTransition::ProcessFresh
                }
            }
            Self::AwaitingComment { .. } => {
                if lower.contains("skip") || lower.contains("пропустить") || lower == "далее"
                {
                    TextTransition::SetComment(String::new())
                } else {
                    TextTransition::SetComment(text.to_string())
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
        let item = PendingItem::new("Test Title".to_string(), KnowledgeType::Book, 12345);
        assert!(!item.id.is_empty());
        assert_eq!(item.source, "telegram");
        assert_eq!(item.status, ContentStatus::Backlog);
        assert_eq!(item.chat_id, 12345);
    }

    #[test]
    fn pending_item_id_should_flatten_slash_from_github_title() {
        // Regression: GitHub owner/repo titles ("owner/repo") used to bake a
        // `/` into the id, committing files into inbox/pending/<owner>/<file>.
        let item = PendingItem::new(
            "andyrewlee/awesome-agent-orchestrators".to_string(),
            KnowledgeType::Link,
            12345,
        );
        assert!(!item.id.contains('/'), "id was: {}", item.id);
        assert!(!item.id.contains('\\'));
        assert!(item.id.contains("-andyrewlee-awesome-a"));
    }

    #[test]
    fn pending_item_id_should_flatten_slash_from_url_path_title() {
        // The same bug occurred with URLs whose detected title contains a
        // slash (e.g. agent_skills/advisor-orchestrator-worker).
        let item = PendingItem::new(
            "agent_skills/advisor-orchestrator-worker".to_string(),
            KnowledgeType::Link,
            12345,
        );
        assert!(!item.id.contains('/'), "id was: {}", item.id);
        assert!(item.id.ends_with("-agent_skills-advisor"));
    }

    #[test]
    fn pending_item_id_should_keep_unicode_letters() {
        let item = PendingItem::new("Проверка ссылки".to_string(), KnowledgeType::Link, 12345);
        assert!(item.id.contains("проверка"));
    }
}
