# 🤖 Wiki LLM Bot

A Cloudflare Worker bot for building a personal wiki/LLM knowledge base. Send links or titles via Telegram — the bot detects the source, lets you choose type/status/rating/comment, and saves YAML files to a GitHub repository for later processing by Hermes.

## ✨ Features

- **📚 Multi-content support**: Books, Movies, Series, Anime, Articles, Courses, GitHub repos, YouTube videos, Tools, Notes
- **🔗 Smart detection**: URL provider detection (GitHub, YouTube, Goodreads, arXiv, etc.) without AI
- **🎯 Simple statuses**: Backlog, Done, Dropped — with context-aware labels (To-read/Read, To-watch/Watched, Planned/Finished, Using)
- **⭐ Rating & comments**: Rate 1-10 and add comments for completed or dropped items
- **🤖 AI-powered analysis** (JSON Schema mode): Cloudflare Workers AI extracts title, author, year, description, and tags with guaranteed structured output
- **🔗 GitHub metadata resolution**: Fetches description, language, stars, and topics via GitHub API (no AI)
- **💾 GitHub integration**: Saves to `<repository>/inbox/pending/` as flat YAML files
- **🖼️ Media support**: Handles photo and PDF uploads with file_id tracking
- **🔍 Deduplication**: Prevents duplicate entries via KV store
- **⌨️ Button-based UI**: All interactions via Telegram reply keyboards

## 🏗 Architecture

```
Telegram → Cloudflare Worker
  ├── Cloudflare KV (state + dedup, 30 min TTL)
  ├── Detector (URL → provider/resource type)
  ├── Resolver (GitHub API → metadata, no AI)
  ├── Cloudflare AI (JSON Schema mode, temperature 0.15)
  ├── Cloudflare Queue (async enrichment)
  └── GitHub API → <repository>/inbox/pending/
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
    ├── github.rs       # GitHub commit to inbox/pending/
    ├── ai.rs           # Cloudflare Workers AI (JSON Schema mode)
    ├── detector.rs     # URL → provider/resource type
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

### Send a GitHub link (with API metadata enrichment)

```
User: https://github.com/tokio-rs/tokio
Bot: 🔗 GitHub: tokio-rs/tokio
     What type?
     [📚 Book]  [🎬 Movie]
     [📺 Series] [🎌 Anime]
     [📄 Article] [🎓 Course]
     [🐙 GitHub] [▶️ YouTube]
     [🛠 Tool]   [📝 Note]
     [📋 Other]

User: 🐙 GitHub
Bot: Resolving via GitHub API...
Bot: 🐙 tokio
     📝 An event-driven, non-blocking I/O platform for Rust
     🔤 Rust | ⭐ 32000
     📌 Status?
     [📋 Backlog] [✅ Done]
     [❌ Dropped] [❌ Cancel]

User: ✅ Done
Bot: Rate 1-10 or skip:

User: ⏭ Skip
Bot: Add a comment or skip:

User: Essential for async Rust
Bot: ✅ Saved: inbox/pending/2026-07-08_tokio.yaml
```

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
     [📄 Article] [🎓 Course]
     [🐙 GitHub] [▶️ YouTube]
     [🛠 Tool]   [📝 Note]
     📋 Other    [❌ Cancel]

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
Bot: 🔗 YouTube: (video)
     What type?
     [📚 Book]  [🎬 Movie]
     ...
     [▶️ YouTube]
     
User: ▶️ YouTube
Bot: ▶️ Status?
     [📋 To-watch] [✅ Watched]
     [❌ Dropped] [❌ Cancel]
```

### Send a photo or PDF

```
User: (photo upload)
Bot: 🖼 Image received
     What type?
     [📚 Book]  [🎬 Movie]
     ...
```

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
status: done
title: "Clean Architecture"
author: "Robert C. Martin"
language: en
year: 2017
stars: null
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

### AI Analysis Details

- **JSON Schema mode**: Workers AI `response_format` with `json_schema` guarantees valid structured output — no manual JSON parsing
- **Model**: `@cf/meta/llama-3.1-8b-instruct-fp8-fast` by default (configurable via `AI_MODEL`)
- **Temperature**: 0.15 (low — deterministic classification, not creative generation)
- **Extracted fields**: `type` (enum), `title` (required), `author`, `year`, `description`, `tags`
- **Human-in-the-loop**: AI result is shown as a suggestion — user confirms or changes type before proceeding

### GitHub Metadata Resolution (no AI)

When a user selects `🐙 GitHub` type, the bot fetches real metadata via GitHub API:
- `title` → actual repository name (not URL slug)
- `description` → repo description
- `language` → primary programming language
- `stars` → star count
- `tags` → repository topics

### Deduplication Methods

- **By URL**: Exact match on the source URL
- **By title**: Exact match on the item title
- **Expired draft detection**: Bot notifies if a draft times out (30 min TTL)

## 🔒 Security

- Only the allowed Telegram username can use the bot
- All secrets stored in Cloudflare Secrets
- No hardcoded credentials
- KV-based deduplication (by title and URL)

## 📄 License

MIT