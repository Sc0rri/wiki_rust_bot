# 🤖 Wiki LLM Bot

A Cloudflare Worker bot for building a personal wiki/LLM knowledge base. Send links, titles, photos, or PDFs via Telegram — the bot saves YAML files to a GitHub repository for later processing by Hermes.

## ✨ Features

- **📚 Content types**: Book, Movie, Series, Anime, Link, Note
- **🔗 Smart URL detection**: GitHub, YouTube, Goodreads, IMDb/Kinopoisk, arXiv, Coursera/Udemy/Stepik, Habr, Wikipedia, etc.
- **🎯 Simple statuses**: Backlog, Done, Dropped — with context-aware labels (To-read/Read, To-watch/Watched)
- **📺 Season tracking**: Series and Anime get an extra season prompt before rating
- **⭐ Rating & comments**: Rate 1-10 and add comments for completed or dropped items
- **🤖 AI-powered analysis** (JSON Schema mode): Cloudflare Workers AI extracts title, author, year, description, and tags with guaranteed structured output
- **🔗 GitHub metadata resolution**: Fetches description, language, stars, and topics via GitHub API (no AI)
- **💾 GitHub integration**: Saves to `<repository>/inbox/pending/` as flat YAML files
- **🖼️ Media archiving**: Photos and PDFs are permanently saved to `<repository>/inbox/assets/` with metadata in inbox/pending/
- **🔁 Forwarded messages**: Automatically saved as Notes without additional prompts
- **🔍 Deduplication**: Prevents duplicate entries via KV store (by title and URL)
- **⌨️ Button-based UI**: All interactions via Telegram reply keyboards
- **🕒 Draft timeout**: State expires after 30 minutes — user is notified instead of silently reinterpreting old input

## 🏗 Architecture

```
Telegram → Cloudflare Worker
  ├── Cloudflare KV (state + dedup, 30 min TTL)
  ├── Detector (URL → provider)
  ├── Resolver (GitHub API → metadata, no AI)
  ├── Cloudflare AI (JSON Schema mode, temperature 0.15)
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
    ├── resolver.rs     # Public API resolvers (GitHub API)
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
npx wrangler kv namespace create STATE_STORE --preview
npx wrangler kv namespace create DEDUP_STORE --preview
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
Bot: 🔗 tokio-rs/tokio
     🔗 https://github.com/tokio-rs/tokio
     📦 GitHub
     ⭐ 32000 · Rust
     Add a comment or skip:

User: ⏭ Skip
Bot: ✅ Saved: inbox/pending/2026-07-08_tokio.yaml
```

GitHub links are auto-detected as `Link` type. The bot fetches real metadata (name, description, language, stars, topics) via GitHub API — no AI needed.

### Send a title (text input with AI analysis + human confirmation)

```
User: Clean Architecture
Bot: 🔍 Analyzing using AI (JSON Schema mode)...

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
     [📋 To-read] [✅ Read]
     [❌ Dropped] [❌ Cancel]

User: ✅ Read
Bot: Rate 1-10 or skip:

User: 9
Bot: Add a comment or skip:

User: ⏭ Skip
Bot: ✅ Saved: inbox/pending/2026-07-08_clean-architecture.yaml
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
Bot: ✅ Saved: inbox/pending/2026-07-08_youtube-video.yaml
```

YouTube links are auto-detected as `Link` type — no status/rating, just an optional comment.

### Send a photo or PDF

```
User: (photo upload)
Bot: Add a comment or skip:
     [⏭ Skip]
     [❌ Cancel]

User: Architecture diagram
Bot: ✅ Saved: inbox/pending/2026-07-08_image-note.yaml
```

The photo/PDF is permanently archived to `inbox/assets/YYYY-MM-DD_slug.{jpg|pdf}` in the same repo.

### Send a forwarded message

```
User: (forwarded text)
Bot: ✅ Saved: inbox/pending/2026-07-08_forwarded-note.yaml
```

Forwarded messages are automatically saved as Notes without any prompts.

### Commands

| Command | Action |
|---------|--------|
| `/start` | Show welcome message |
| `/cancel` | Cancel current draft and clear state |

## 📁 Saved File Format (YAML)

Each item is saved as a flat YAML file under `inbox/pending/` with the filename format `YYYY-MM-DD_slug.yaml`:

```yaml
---
id: 20260707153000-clean-architecture
created: 2026-07-08
source: telegram
provider: goodreads
url: "https://www.goodreads.com/book/show/123"
type: book
status: read
title: "Clean Architecture"
author: "Robert C. Martin"
year: 2017
rating: 9
comment: "Great book on software architecture"
description: "A guide to software design and architecture"
tags:
  - "architecture"
  - "ddd"
  - "clean code"
processed: false
---
```

Fields like `author`, `year`, `language`, `stars`, `rating`, `comment`, `description`, and `season` are omitted when empty.

## 🎯 Content Types & Statuses

The bot recognizes five content types. Only **Book**, **Movie**, **Series**, and **Anime** get the full status/rating/comment flow — these are the only types where "did I finish it, was it good" is a meaningful question. Everything else is either a **Link** (URL) or a **Note** (plain text / media).

### Statuses (context-aware labels)

| Status | Book | Movie/Series/Anime |
|--------|------|--------------------|
| 📋 Backlog | To-read | To-watch |
| ✅ Done | Read | Watched |
| ❌ Dropped | Dropped | Dropped |

### Content Types

| Type | Button | Flow |
|------|--------|------|
| 📚 Book | Book | Status → Rating → Comment |
| 🎬 Movie | Movie | Status → Rating → Comment |
| 📺 Series | Series | Status → Season → Rating → Comment |
| 🎌 Anime | Anime | Status → Season → Rating → Comment |
| 🔗 Link | (auto) | Comment only (no status/rating) |
| 📝 Note | Note | Comment only (no status/rating) |

### AI Analysis Details

- **JSON Schema mode**: Workers AI `response_format` with `json_schema` guarantees valid structured output — no manual JSON parsing
- **Model**: `@cf/meta/llama-3.1-8b-instruct-fp8-fast` by default (configurable via `AI_MODEL`)
- **Temperature**: 0.15 (low — deterministic classification, not creative generation)
- **Extracted fields**: `type` (enum: book/movie/series/anime/note), `title` (required), `author`, `year`, `description`, `tags`
- **Human-in-the-loop**: AI result is shown as a suggestion — user confirms or changes type before proceeding

### GitHub Metadata Resolution (no AI)

When a user sends a GitHub link, the bot fetches real metadata via GitHub API:
- `title` → actual repository name (not URL slug)
- `description` → repo description
- `language` → primary programming language
- `stars` → star count
- `tags` → repository topics

### Deduplication Methods

- **By URL**: Exact match on the source URL
- **By title**: Case-insensitive exact match on the item title
- **Expired draft detection**: Bot notifies if a draft times out (30 min TTL)

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
2. The file is permanently saved to `<repository>/inbox/assets/YYYY-MM-DD_slug.{jpg|pdf}`
3. A metadata entry is saved to `inbox/pending/` with an `asset:` tag pointing to the archived file
4. If the download or GitHub upload fails, the bot falls back to tagging the Telegram `file_id` so the item is still captured

This is important because Telegram `file_id`s can expire and are only resolvable within the same bot token.

## 🔒 Security

- Only the allowed Telegram username can use the bot
- All secrets stored in Cloudflare Secrets
- No hardcoded credentials
- KV-based deduplication (by title and URL)

## 📄 License

MIT