# 🤖 Wiki LLM Bot

A Cloudflare Worker bot for building a personal wiki/LLM knowledge base. Send links or titles via Telegram — the bot detects the source, lets you choose type/status/rating/comment, and saves YAML files to a GitHub repository for later processing by Hermes.

## ✨ Features

- **📚 Multi-content support**: Books, Movies, Series, Anime, Articles, Courses, Papers, GitHub, YouTube, Tools, Ideas, Notes
- **🔗 Smart detection**: URL provider detection (GitHub, YouTube, Goodreads, arXiv, etc.) without AI
- **🎯 Granular statuses**: To-read/Read, To-watch/Watched, Planned/In progress/Finished, Dropped, Using/Library/Interesting
- **⭐ Rating & comments**: Rate 1-10 and add comments for consumed content
- **🤖 AI-powered analysis**: Cloudflare Workers AI for text-only inputs (optional)
- **💾 GitHub integration**: Saves to `<repository>/inbox/pending/` as flat YAML files
- **🖼️ Media support**: Handles photo and PDF uploads
- **🔍 Deduplication**: Prevents duplicate entries via KV store
- **⌨️ Button-based UI**: All interactions via Telegram reply keyboards

## 🏗 Architecture

```
Telegram → Cloudflare Worker
  ├── Cloudflare KV (state + dedup)
  ├── Detector (URL → provider/resource type)
  ├── Cloudflare AI (optional, for text-only inputs)
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
    ├── lib.rs          # HTTP entry + Queue consumer
    ├── app.rs          # Webhook handler + state machine
    ├── telegram.rs     # Telegram API types + service
    ├── github.rs       # GitHub API (commit to inbox/pending/)
    ├── ai.rs           # Cloudflare AI analysis (optional)
    ├── detector.rs     # URL → provider/resource detection
    ├── parser.rs       # Slugify, filename generation
    ├── state.rs        # UserState, PendingItem, KnowledgeType/Status
    ├── dedup.rs        # Deduplication service
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

### Send a link (URL detection)

```
User: https://github.com/tokio-rs/tokio
Bot: 🔗 GitHub: tokio
     What type?
     [📚 Book]  [🎬 Movie]
     [📺 Series] [🎌 Anime]
     [📄 Article] [🎓 Course]
     [📑 Paper]  [🐙 GitHub]
     [▶️ YouTube] [🛠 Tool]
     [💡 Idea]   [📝 Note]
     [📋 Other]

User: 🛠 Tool
Bot: 🛠 Status?
     [⭐ Using] [📚 Library] [💡 Interesting]
     [❌ Cancel]

User: ⭐ Using
Bot: Rate 1-10 or skip:

User: 9
Bot: Add a comment or skip:

User: Essential for async Rust
Bot: 🛠 tokio
     🔗 https://github.com/tokio-rs/tokio
     📦 GitHub
     📌 Status: Using
     ⭐ 9/10
     💬 "Essential for async Rust"
     
     Save?
     [✅ Save] [❌ Cancel]

User: ✅ Save
Bot: ✅ Saved: inbox/pending/2026-07-07_tokio.yaml
```

### Send a title (text input)

```
User: Clean Architecture
Bot: Detected title. What type?
     [📚 Book]  [🎬 Movie]
     ...

User: 📚 Book
Bot: 📚 Status?
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
Bot: 🔗 YouTube: video title
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
language: rust
year: 2017
rating: 8
comment: "Great book on software architecture"
tags:
  - "architecture"
  - "ddd"
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
| 📑 Paper | Saved directly |
| 🐙 GitHub | Using, Library, Interesting |
| ▶️ YouTube | To-watch, Watched, Dropped |
| 🛠 Tool | Using, Library, Interesting |
| 💡 Idea | Confirm/save directly |
| 📝 Note | Confirm/save directly |
| 📋 Other | Saved directly |

## 🔒 Security

- Only the allowed Telegram username can use the bot
- All secrets stored in Cloudflare Secrets
- No hardcoded credentials
- KV-based deduplication (by title and URL)

## 📄 License

MIT