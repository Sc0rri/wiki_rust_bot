# 🤖 Wiki LLM Bot

A Cloudflare Worker bot for building a personal wiki/LLM knowledge base. Send links or titles via Telegram — the bot analyzes them with AI, lets you choose type/status/category, and saves YAML-frontmatter markdown files to a GitHub repository for later processing by Hermes.

## ✨ Features

- **📚 Multi-content support**: Books, Movies, Series, Anime, Articles, Courses, Papers, Tools, PDFs, Images, Ideas, Notes
- **🔗 Link or text**: Send URLs or just titles → AI auto-detects type
- **🎯 Granular statuses**: To-read/Read, To-watch/Watched, Planned/In progress/Finished, Dropped, Using/Library/Interesting
- **📂 Categories**: Programming, News, Education, Research, Gaming, etc. (for articles, PDFs, images)
- **🤖 AI-powered analysis**: Cloudflare Workers AI (`@cf/meta/llama-3.2-11b-instruct`) extracts metadata
- **💾 GitHub integration**: Saves to `<repository>/inbox/pending/`
- **🖼️ Media support**: Handles photo and PDF uploads
- **🔍 Deduplication**: Prevents duplicate entries via KV store
- **⌨️ Button-based UI**: All interactions via Telegram reply keyboards

## 🏗 Architecture

```
Telegram → Cloudflare Worker
  ├── Cloudflare KV (state + dedup)
  ├── Cloudflare AI (LLM analysis)
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
    ├── ai.rs           # Cloudflare AI analysis
    ├── parser.rs       # URL/text parsing
    ├── state.rs        # UserState, PendingItem, ContentType/Status
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

### 3. Create Cloudflare Queue

```bash
npx wrangler queue create wiki-inbox-queue
```

### 4. Configure secrets

```bash
npx wrangler secret put BOT_TOKEN
npx wrangler secret put ALLOWED_USERNAME
npx wrangler secret put GITHUB_TOKEN
npx wrangler secret put GITHUB_REPO
npx wrangler secret put AI_MODEL
# Default: @cf/meta/llama-3.2-11b-instruct
```

### 5. Deploy

```bash
npx wrangler deploy
```

### 6. Set Telegram webhook

```bash
curl -F "url=https://<YOUR_WORKER_URL>/webhook" \
  https://api.telegram.org/bot<YOUR_BOT_TOKEN>/setWebhook
```

## 📖 Usage

### Send a link

```
User: https://habr.com/ru/articles/123456/
Bot: 📄 Article detected. Category?
     [💻 Programming] [📰 News]
     [🧠 Concept]     [📚 Education]
     [🎮 Gaming]      [🎬 Entertainment]
     [📋 Other]        [❌ Cancel]

User: 💻 Programming
Bot: Status?
     [📥 Inbox] [⭐ Important]
     [❌ Cancel]

User: 📥 Inbox
Bot: ⏳ Saving...
Bot: ✅ Saved: inbox/pending/article/inbox/2026-07-07_docker-networking.md
```

### Send a title

```
User: Clean Architecture
Bot: Detected title. Choose type:
     [📚 Book]  [🎬 Movie]
     [📺 Series] [🎌 Anime]
     [📄 Article] [🎓 Course]
     [📑 Paper]  [🛠 Tool]
     [📕 PDF]    [🖼 Image]
     [💡 Idea]   [📝 Note]
     [📋 Other]

User: 📚 Book
Bot: 📚 Status?
     [📋 To-read] [✅ Read]
     [❌ Dropped] [❌ Cancel]

User: 📋 To-read
Bot: Add details? (author, year, tags) or skip:
     [⏭ Skip] [❌ Cancel]

User: ⏭ Skip
Bot: ⏳ Saving...
Bot: ✅ Saved: inbox/pending/book/to-read/2026-07-07_clean-architecture.md
```

### Send a YouTube link

```
User: https://youtu.be/xxxxx
Bot: ⏳ Processing link...
Bot: ✅ Saved: inbox/pending/movie/to-watch/2026-07-07_video-title.md
```

### Send a photo or PDF

```
User: (photo upload)
Bot: 🖼 Image received. What is it?
     [📚 Book cover] [📝 Notes]
     [📊 Diagram]    [📄 Document]
     [📋 Other]
```

## 📁 Saved File Format (YAML + Markdown)

Each item is saved as a markdown file under `inbox/pending/` with YAML frontmatter:

```yaml
---
type: book                   # Content type
title: "Clean Architecture"  # Title
category: "Programming"      # Optional category
author: "Robert C. Martin"   # Optional author
year: 2017                   # Optional year
url: ""                      # Optional source URL
status: to-read              # Current status
source: telegram             # Source (telegram)
created: 2026-07-07          # Date added
processed: false             # Pending Hermes processing
tags:                        # Optional tags
  - "architecture"
  - "ddd"
---
```

## 🎯 Content Types & Statuses

| Type | Available Statuses |
|------|-------------------|
| 📚 Book | To-read, Read, Dropped |
| 🎬 Movie | To-watch, Watched, Dropped |
| 📺 Series | To-watch, Watched, Dropped |
| 🎌 Anime | To-watch, Watched, Dropped |
| 📄 Article | Category → Inbox/Important |
| 🎓 Course | Planned, In progress, Finished, Dropped |
| 📑 Paper | Saved directly |
| 🛠 Tool | Using, Library, Interesting |
| 📕 PDF | Category → Inbox/Important |
| 🖼 Image | Category → Inbox/Important |
| 💡 Idea | Confirm/save directly |
| 📝 Note | Confirm/save directly |
| 📋 Other | Saved directly |

### Categories (for Articles, PDFs, Images)

**Articles**: Programming, News, Concept, Education, Gaming, Entertainment  
**PDFs**: Programming, Research, Book, Manual  
**Images**: Book cover, Notes, Diagram, Document

## 📁 Inbox Structure

```
inbox/pending/
├── article/inbox/
├── article/important/
├── book/to-read/
├── book/read/
├── book/dropped/
├── movie/to-watch/
├── movie/watched/
├── course/planned/
├── course/in-progress/
├── course/finished/
├── tool/using/
├── tool/library/
├── tool/interesting/
├── paper/
├── pdf/inbox/
├── image/inbox/
└── ...
```

## 🔒 Security

- Only the allowed Telegram username can use the bot
- All secrets stored in Cloudflare Secrets
- No hardcoded credentials
- KV-based deduplication

## 📄 License

MIT