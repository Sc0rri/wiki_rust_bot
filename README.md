# 🤖 Wiki LLM Bot

A Cloudflare Worker bot for building a personal wiki/LLM knowledge base. Send links, titles, photos, or PDFs via Telegram — the bot saves YAML files to a GitHub repository for later processing by Hermes.

## ✨ Features

- **📚 Content types**: Book, Movie, Series, Anime, Link, Note
- **🔗 Smart URL detection**: GitHub, YouTube, Goodreads, IMDb/Kinopoisk, arXiv, Coursera/Udemy/Stepik, Habr, Wikipedia, etc.
- **🎯 Statuses**: Universal Telegram buttons — Backlog, Done, Dropped. Saved/displayed metadata is type-aware: To-read/Read for books, To-watch/Watched for movies/series/anime
- **📺 Season tracking**: Series and Anime get an extra season prompt before rating
- **⭐ Rating**: Rate 1-10 for Done or Dropped statuses (Backlog skips rating)
- **💬 Comment**: Optional follow-up comment after rating
- **🤖 AI analysis** (JSON Schema mode): Cloudflare Workers AI classifies text input and can add summaries/tags for saved links
- **🔗 Provider enrichment**: GitHub links get repository metadata via GitHub API; YouTube links use oEmbed; generic pages can use HTML title/meta extraction
- **💾 GitHub integration**: Saves to `<repository>/inbox/pending/` as flat YAML files
- **🖼️ Media archiving**: Photos and PDFs are archived to `<repository>/inbox/assets/` when possible, with metadata in inbox/pending/
- **🔁 Forwarded messages**: Automatically saved as Notes without additional prompts
- **🔍 Deduplication**: Prevents duplicate entries via KV store by title; URL keys are recorded for saved links
- **⌨️ Guided Telegram UI**: Reply keyboards for choices, with free-text input for season, rating, and comments
- **🕒 Draft timeout**: Draft state expires after 30 minutes; likely expired rating replies are reported instead of being reprocessed as new input

## 🏗 Architecture

```
Telegram → Cloudflare Worker
  ├── Cloudflare KV (state + dedup, 30 min TTL)
  ├── Detector (URL → provider)
  ├── Resolver (GitHub API, YouTube oEmbed, generic HTML metadata)
  ├── Cloudflare AI (JSON Schema mode, temperature 0.15) — text classification + link summary/tags
  └── GitHub API → <repository>/inbox/pending/
          ├── <repository>/inbox/assets/  (for photos/PDFs)
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
    ├── github.rs       # GitHub commit to inbox/pending/ + inbox/assets/
    ├── ai.rs           # Cloudflare Workers AI (JSON Schema mode)
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
# Optional: npx wrangler secret put AI_MODEL
# Default: @cf/meta/llama-3.1-8b-instruct-fp8-fast
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

### Send a title (text input — AI classifies, user confirms)

```
User: Clean Architecture
Bot: 🤖 Looks like: 📚 Book
     👤 Robert C. Martin
     📅 2017
     Confirm or change type?
     [✅ Confirm]
     [📚 Book] [🎬 Movie]
     [📺 Series] [🎌 Anime]
     [📝 Note]  [❌ Cancel]

User: ✅ Confirm
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

If AI misidentifies the type — user just taps the correct button instead of Confirm.

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
processed: false
---
```

Optional fields (`author`, `year`, `language`, `stars`, `rating`, `comment`, `season`, `raw_text`) are omitted when empty. `tags` is written as an empty list when there are no tags. Link summaries/descriptions are shown in chat previews but are not currently written to YAML.

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
Bot saves as `Link` type. It resolves provider metadata where possible, can ask AI for a short summary and topic tags, then asks only for an optional comment. URL classification itself is rule-based.

### Text messages
AI classifies the text as one of: book, movie, series, anime, or note.  
User can confirm the suggestion or pick a different type.  
- Book/Movie/Series/Anime → full status → (season) → rating → comment flow  
- Note → saved immediately

### Photos / PDFs
Saved as `Note` type. Caption is used as title if present.  
The bot archives the file to `inbox/assets/` when possible to avoid Telegram file_id expiration.  
Metadata (SHA-256, MIME type, dimensions) is saved alongside the item when available.

### Forwarded messages  
Automatically saved as `Note` with a `forwarded` tag — no prompts.

### AI Analysis Details

- **JSON Schema mode**: Workers AI `response_format` with `json_schema` requests structured output; the bot also has a defensive JSON extraction fallback for models that return text
- **Model**: `@cf/meta/llama-3.1-8b-instruct-fp8-fast` by default (configurable via `AI_MODEL`)
- **Temperature**: 0.15 (low — deterministic classification, not creative generation)
- **Classifies into**: `type` (enum: book/movie/series/anime/note), `title` (required), `author`, `year`, `description`, `tags`
- **Human-in-the-loop**: AI result is shown as a suggestion — user confirms or changes type before proceeding
- **URLs are rule-detected first**: Links are not AI-classified, but resolved link metadata can be sent to AI for summary and topic tags

### Link Enrichment

When a user sends a GitHub link, the bot fetches repository metadata via GitHub API:
- `title` → actual repository name (not URL slug)
- `description` → repo description used for the chat preview and AI summary context
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
