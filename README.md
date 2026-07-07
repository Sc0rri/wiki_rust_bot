# 🤖 Wiki LLM Bot

A Cloudflare Worker bot for building a personal wiki/LLM knowledge base. Send links or titles via Telegram — the bot detects the source, lets you choose type/status/rating/comment, and saves YAML files to a GitHub repository for later processing by Hermes.

## ✨ Features

- **📚 Multi-content support**: Books, Movies, Series, Anime, Articles, Courses, GitHub repos, YouTube videos, Tools, Notes
- **🔗 Smart detection**: URL provider detection (GitHub, YouTube, Goodreads, arXiv, etc.) without AI
- **🎯 Granular statuses**: To-read/Read, To-watch/Watched, Planned/In progress/Finished, Dropped, Using/Library/Interesting
- **⭐ Rating & comments**: Rate 1-10 and add comments for consumed content
- **🤖 AI-powered analysis**: Cloudflare Workers AI extracts title, author, year, description, and tags from text inputs
- **🔗 GitHub metadata resolution**: Fetches description, language, stars, and topics via GitHub API (no AI)
- **💾 GitHub integration**: Saves to `<repository>/inbox/pending/` as flat YAML files
- **🖼️ Media support**: Handles photo and PDF uploads with file_id tracking
- **🔍 Deduplication**: Prevents duplicate entries via KV store
- **⌨️ Button-based UI**: All interactions via Telegram reply keyboards

## 🏗 Architecture

```
Telegram → Cloudflare Worker
  ├── Cloudflare KV (state + dedup)
  ├── Detector (URL → provider/resource type)
  ├── Resolver (GitHub API → metadata, no AI)
  ├── Cloudflare AI (for text-only inputs → tags + description)
  ├── Cloudflare Queue (async processing)
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
    ├── ai.rs           # Cloudflare Workers AI analysis
    ├── detector.rs     # URL → provider/resource type
    ├── resolver.rs     # Public API resolvers (GitHub, etc.)
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
# Optional: npx wrangler secret put AI_MODEL (default: @cf/meta/llama-3.2-11b-instruct)
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

### Send a GitHub link

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
Bot: Fetching repo info...
Bot: 🐙 tokio
     🔗 https://github.com/tokio-rs/tokio
     📦 GitHub
     📝 An event-driven, non-blocking I/O platform for Rust
     🔤 Rust | ⭐ 32000
     📌 Status?
     [⭐ Using] [📚 Library] [💡 Interesting]
     [❌ Cancel]

User: 📚 Library
Bot: Rate 1-10 or skip:

User: ⏭ Skip
Bot: Add a comment or skip:

User: Essential for async Rust
Bot: 🐙 tokio
     🔗 https://github.com/tokio-rs/tokio
     📦 GitHub
     🔤 Rust | ⭐ 32000
     📌 Status: Library
     💬 "Essential for async Rust"
     
     Save?
     [✅ Save] [❌ Cancel]

User: ✅ Save
Bot: ✅ Saved: inbox/pending/2026-07-07_tokio.yaml
```

### Send a title (text input with AI)

```
User: Clean Architecture
Bot: 🔍 Analyzing...
Bot: 📚 Clean Architecture
     👤 Robert C. Martin (2017)
     📌 Status?
     [📋 To-read] [✅ Read]
     [❌ Dropped] [❌ Cancel]

User: 📋 To-read
Bot: Rate 1-10 or skip:

User: 8
Bot: Add a comment or skip:

User: ⏭ Skip
Bot: 📚 Clean Architecture
     📌 Status: To-read
     ⭐ 8/10
     
     Save?
     [✅ Save] [❌ Cancel]

User: ✅ Save
Bot: ✅ Saved: inbox/pending/2026-07-07_clean-architecture.yaml
```

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
created: 2026-07-07
source: telegram
provider: goodreads
url: "https://www.goodreads.com/book/show/123"
type: book
status: to-read
title: "Clean Architecture"
author: "Robert C. Martin"
language: en
year: 2017
stars: null
rating: 8
comment: "Great book on software architecture"
description: "A guide to software design and architecture"
tags:
  - "architecture"
  - "ddd"
  - "clean code"
processed: false
---
```

## 🎯 Content Types & Statuses

| Type | Available Statuses |
|------|-------------------|
| 📚 Book | To-read, Read, Dropped |
| 🎬 Movie | To-watch, Watched, Dropped |
| 📺 Series | To-watch, Watched, Dropped |
| 🎌 Anime | To-watch, Watched, Dropped |
| 📄 Article | To-read, Read, Dropped |
| 🎓 Course | Planned, In progress, Finished, Dropped |
| 🐙 GitHub | Using, Library, Interesting |
| ▶️ YouTube | To-watch, Watched, Dropped |
| 🛠 Tool | Using, Library, Interesting |
| 📝 Note | Confirm/save directly |
| 📋 Other | Saved directly |

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