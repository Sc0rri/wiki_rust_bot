# 🤖 Wiki Bot

A Cloudflare Worker bot for building a personal wiki knowledge base. Send links, titles, photos, or PDFs via Telegram — the bot saves YAML files to a GitHub repository for later processing by Hermes.

## ✨ Features

- **📚 Content types**: Book, Movie, Series, Anime, Link, Note
- **🔗 Smart URL detection**: GitHub, YouTube, Goodreads, IMDb/Kinopoisk, arXiv, Coursera/Udemy/Stepik, Habr, Wikipedia, etc.
- **🎯 Statuses**: Universal Telegram buttons — Backlog, Done, Dropped. Saved/displayed metadata is type-aware: To-read/Read for books, To-watch/Watched for movies/series/anime
- **📺 Season tracking**: Series and Anime get an extra season prompt before rating
- **⭐ Rating**: Rate 1-10 for Done or Dropped statuses (Backlog skips rating)
- **💬 Comment**: Optional follow-up comment after rating
- **🔗 Provider enrichment**: GitHub links get repository metadata via GitHub API; YouTube links use oEmbed; generic pages can use HTML title/meta extraction
- **💾 GitHub integration**: Saves to `<repository>/inbox/pending/` as flat YAML files
- **🖼️ Media archiving**: Photos and PDFs are archived to `<repository>/inbox/assets/` when possible, with metadata in inbox/pending/
- **🔁 Forwarded messages**: Automatically saved as Notes without additional prompts
- **🔍 Deduplication**: Prevents duplicate entries via KV store by title; URL keys are recorded for saved links
- **⌨️ Guided Telegram UI**: Reply keyboards for choices, with free-text input for season, rating, and comments
- **🕒 Draft timeout**: Draft state expires after 30 minutes; likely expired rating replies are reported instead of being reprocessed as new input
- **💬 Clarification replies**: When an external script sends a clarifying question with `[ref:<id>]` marker, the user's reply is saved to `inbox/pending/<id>.reply.yaml`
- **📝 Telegram chat_id**: Each saved item includes the Telegram `chat_id` so external scripts can send follow-up questions back to the right chat
- **📋 Debug logging**: Telegram webhook events, incoming messages, and runtime errors are written to a single shared daily log at `inbox/logs/YYYY-MM-DD.log` for debugging
- **🔒 YAML safety**: All string fields are properly escaped (backslash, quotes, newlines, tabs) to prevent YAML parsing issues

## 🏗 Architecture

```
Telegram → Cloudflare Worker
  ├── Cloudflare KV (state + dedup, 30 min TTL)
  ├── Detector (URL → provider)
  ├── Resolver (GitHub API, YouTube oEmbed, generic HTML metadata)
  └── GitHub API → <repository>/inbox/
          ├── pending/        (YAML items + reply files)
          ├── assets/         (photos/PDFs)
          ├── logs/           (shared daily debug logs, YYYY-MM-DD.log)
          └── [Hermes] → LLM wiki
```

## 📂 Project Structure

```
├── Cargo.toml
├── wrangler.toml
├── README.md
└── src/
    ├── lib.rs          # HTTP entry + module declarations
    ├── app.rs          # Webhook handler + state machine
    ├── telegram.rs     # Telegram API types + service
    ├── github.rs       # GitHub commit to inbox/pending/ + inbox/assets/ + inbox/logs/ + reply files
    ├── detector.rs     # URL → provider
    ├── resolver.rs     # Provider/web metadata resolvers
    ├── parser.rs       # Slugify, filename generation
    ├── state.rs        # UserState, PendingItem, KnowledgeType/Status
    ├── dedup.rs        # KV-based deduplication
    └── logger.rs       # Logging utilities
```

## 🚀 Setup

### 1. Clone and build

```bash
git clone https://github.com/Sc0rri/wiki_rust_bot.git
cd wiki_rust_bot
```

### 2. Create Cloudflare KV namespaces

```bash
npx wrangler kv namespace create STATE_STORE
npx wrangler kv namespace create DEDUP_STORE
```

Update `wrangler.toml` with the namespace IDs.

### 3. Configure secrets

```bash
npx wrangler secret put BOT_TOKEN
npx wrangler secret put ALLOWED_USERNAME
npx wrangler secret put GITHUB_TOKEN
npx wrangler secret put GITHUB_REPO
# Optional: disable GitHub log writes entirely
# npx wrangler secret put LOG_TO_FILE
# Set value to false to disable the shared daily log file
```

### 4. Deploy

```bash
npx wrangler deploy
```

### 5. Set Telegram webhook

```bash
curl -F "url=https://<YOUR_WORKER_URL>/webhook" \
  https://api.telegram.org/bot<YOUR_BOT_TOKEN>/setWebhook
```

## 📖 Usage

### Send a GitHub link (auto-detected as Link with GitHub enrichment)

```
User: https://github.com/tokio-rs/tokio
Bot: 🔗 tokio
     🔗 https://github.com/tokio-rs/tokio
     📦 GitHub
     ⭐ 32000 · Rust
     Add a comment or skip:

User: ⏭ Skip
Bot: ✅ Saved:
     inbox/pending/2026-07-08_1530_tokio.yaml
```

All URLs are saved as `Link` type — no status or rating prompt, just an optional comment. GitHub links are enriched with repository metadata via GitHub API; YouTube links, articles, and any other URLs use the same flow.

### Send a title (text input — user picks type)

```
User: Clean Architecture
Bot: What type?
     [📚 Book] [🎬 Movie]
     [📺 Series] [🎌 Anime]
     [📝 Note]  [❌ Cancel]

User: 📚 Book
Bot: 📚 Status?
     [📋 Backlog] [✅ Done]
     [❌ Dropped] [❌ Cancel]

User: ✅ Done
Bot: Rate 1-10 or skip:

User: 9
Bot: Add a comment or skip:

User: ⏭ Skip
Bot: ✅ Saved:
     inbox/pending/2026-07-08_1530_clean-architecture.yaml
```

### Send a YouTube link

```
User: https://youtu.be/xxxxx
Bot: 🔗 YouTube video
     🔗 https://youtu.be/xxxxx
     📦 YouTube
     Add a comment or skip:

User: ⏭ Skip
Bot: ✅ Saved:
     inbox/pending/2026-07-08_1530_youtube-video.yaml
```

YouTube links (and all other URLs) are `Link` type — no status/rating prompt, just an optional comment.

### Send a photo or PDF

```
User: (photo upload)
Bot: 📎 File archived to inbox/assets/.
     Add a comment or skip:
     [⏭ Skip]
     [❌ Cancel]

User: Architecture diagram
Bot: ✅ Saved:
     inbox/pending/2026-07-08_1530_image-note.yaml
```

The photo/PDF is archived to `inbox/assets/YYYY-MM-DD_HHMM_slug.{jpg|pdf}` in the same repo. If archiving fails, the bot still saves a note with the Telegram `file_id` in tags and warns in chat.

### Send a forwarded message

```
User: (forwarded text)
Bot: ✅ Saved:
     inbox/pending/2026-07-08_1530_forwarded-note.yaml
```

Forwarded messages are automatically saved as Notes without any prompts.

### Commands

| Command | Action |
|---------|--------|
| `/start` | Show welcome message |
| `/cancel` | Cancel current draft and clear state |
| `/clear` | Clear dedup store — treat all previously saved items as new again |

## 📁 Saved File Format (YAML)

Each item is saved as a flat YAML file under `inbox/pending/` with the filename format `YYYY-MM-DD_HHMM_slug.yaml`:

```yaml
---
id: 20260707153000-tokio-rs/tokio
created: 2026-07-08
source: telegram
provider: github
url: "https://github.com/tokio-rs/tokio"
type: link
status: backlog
title: "tokio"
raw_text: "https://github.com/tokio-rs/tokio"
language: Rust
stars: 32000
comment: "Async runtime to revisit"
tags:
  - "rust"
  - "async"
---
```

Each item includes a `chat_id` field (the Telegram chat ID it was submitted from), so external scripts can send follow-up questions back to the right chat.

Optional fields (`author`, `year`, `language`, `stars`, `rating`, `comment`, `season`, `raw_text`, `chat_id`) are omitted when empty. `tags` is written as an empty list when there are no tags. Link summaries/descriptions are shown in chat previews but are not currently written to YAML.

> **YAML safety**: All string fields (`title`, `raw_text`, `author`, `comment`, `url`, `tags`) are escaped via `yaml_quote()` — backslashes, double quotes, carriage returns, newlines, and tabs are encoded as `\\`, `\"`, `\r`, `\n`, `\t` respectively. This ensures multi-line text (e.g. forwarded reviews) is stored as a single-line YAML value without breaking the frontmatter.

For media items (photos/PDFs), additional metadata is saved when available:
- `asset_sha256` — SHA-256 hash of the archived file
- `asset_mime` — MIME type (e.g. `image/jpeg`, `application/pdf`)
- `asset_width`, `asset_height` — image dimensions (photos only)

## 🎯 Content Types & Flows

The bot recognizes six content types. Only **Book**, **Movie**, **Series**, and **Anime** get the full flow — these are the only types where "did I finish it, was it good" is meaningful. Everything else is either a **Link** (any URL) or a **Note** (plain text / media / forwarded message).

### Type flows

| Type | Button | Flow |
|------|--------|------|
| 📚 Book | Book | Status → Rating → Comment |
| 🎬 Movie | Movie | Status → Rating → Comment |
| 📺 Series | Series | Status → Season → Rating → Comment |
| 🎌 Anime | Anime | Status → Season → Rating → Comment |
| 🔗 Link | (auto) | Comment only (no status/rating prompt) |
| 📝 Note | Note | Saved immediately for text; media notes ask for an optional comment |

### Status buttons and saved values

Telegram always shows the same status buttons for Book/Movie/Series/Anime:

| Button | Meaning |
|--------|---------|
| 📋 Backlog | Planned for later |
| ✅ Done | Finished |
| ❌ Dropped | Abandoned |

When a media item is previewed or saved to YAML, the selected status is rendered with type-specific labels. Link/Note items do not ask for status, but the YAML still includes the default `status: backlog` field.

| Status | Book | Movie / Series / Anime | Link / Note |
|--------|------|------------------------|-------------|
| 📋 Backlog | To-read | To-watch | Saved as `backlog` by default |
| ✅ Done | Read | Watched | Not prompted |
| ❌ Dropped | Dropped | Dropped | Not prompted |

- **Backlog** → skips rating; Series/Anime still ask for season before comment
- **Done / Dropped** → asks for rating, then comment

## 🔍 How each input type is processed

### URLs (any)
Bot saves as `Link` type. It resolves provider metadata where possible (GitHub API, YouTube oEmbed, HTML title/meta extraction), then asks only for an optional comment. URL classification itself is rule-based.

### Text messages
User picks the type manually: book, movie, series, anime, or note.  
- Book/Movie/Series/Anime → full status → (season) → rating → comment flow  
- Note → saved immediately

### Photos / PDFs
Saved as `Note` type. Caption is used as title if present.  
The bot archives the file to `inbox/assets/` when possible to avoid Telegram file_id expiration.  
Metadata (SHA-256, MIME type, dimensions) is saved alongside the item when available.

### Forwarded messages  
Automatically saved as `Note` with a `forwarded` tag — no prompts.

## 📋 Debug Logging

Every Telegram request and bot event is written into a single daily file at `inbox/logs/YYYY-MM-DD.log` in the GitHub repository. The same file contains:
- webhook receipts from Telegram
- incoming message summaries
- normal runtime logs
- GitHub/Telegram errors rendered as readable blocks with separators

Example:

```
2026-07-30T11:54:44.123Z [debug] telegram.message.incoming - chat_id=123 from=Some(456) text_preview="..." has_text=true has_caption=false has_photo=false has_document=false has_reply=false has_forward=false
2026-07-30T11:54:45.555Z [info] telegram.webhook.received - path=/webhook bytes=982
=== ERROR github.api.post_failed ===
2026-07-30T11:54:45.555Z [error] github.api.post_failed - status=500 body=bad gateway
================================
```

The log entry includes:
- `chat_id` — the chat the message came from
- `from` — the sender's Telegram user ID
- `text_preview` — first 50 characters of the message text
- Boolean flags for: `has_text`, `has_caption`, `has_photo`, `has_document`, `has_reply`, `has_forward`

Logs are best-effort, but the current implementation keeps webhook events, normal bot logs, and error blocks in one shared day file to make debugging easier.

### Link Enrichment

When a user sends a GitHub link, the bot fetches repository metadata via GitHub API:
- `title` → actual repository name (not URL slug)
- `description` → repo description used for the chat preview
- `language` → primary programming language
- `stars` → star count
- `tags` → repository topics

YouTube links use the public oEmbed endpoint for title/author when available. Other web pages can be fetched for `<title>` and meta description.

### Deduplication Methods

- **By title**: Case-insensitive exact match on the item title
- **URL bookkeeping**: Saved link URLs are marked in KV and cleared by `/clear`, but duplicate checks currently use the title key
- **Expired draft detection**: Numeric replies after state expiry are treated as likely stale ratings and reported to the user

### Supported URL Providers

| Provider | Detected by |
|----------|-------------|
| 🐙 GitHub | `github.com` |
| ▶️ YouTube | `youtube.com`, `youtu.be` |
| 📚 Goodreads | `goodreads.com` |
| 🎬 IMDb | `imdb.com`, `kinopoisk.ru` |
| 📄 arXiv | `arxiv.org` |
| 🎓 Coursera / Udemy / Stepik | `coursera.org`, `udemy.com`, `stepik.org` |
| 📰 Habr | `habr.com` |
| 🌐 Wikipedia | `wikipedia.org` |
| 🌍 Generic | Everything else → `Web` |

### Media Archiving Details

When a user sends a photo or PDF:
1. The bot downloads the file from Telegram's servers via `getFile` API
2. If archiving succeeds, the file is saved to `<repository>/inbox/assets/YYYY-MM-DD_HHMM_slug.{jpg|pdf}`
3. A metadata entry is saved to `inbox/pending/` with `asset_sha256`, `asset_mime`, and dimensions when available
4. If the download or GitHub upload fails, the bot falls back to tagging the Telegram `file_id` so the item is still captured

This is important because Telegram `file_id`s can expire and are only resolvable within the same bot token.

## 🔒 Security

- Only the allowed Telegram username can use the bot
- All secrets stored in Cloudflare Secrets
- No hardcoded credentials
- KV-based deduplication by title

## 📄 License

MIT
