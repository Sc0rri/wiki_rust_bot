# 🤖 Wiki LLM Bot

A Cloudflare Worker bot for building a personal wiki/LLM knowledge base. Send links or titles of books, movies, series, and anime via Telegram, and the bot will save them to a GitHub repository for later processing by Hermes.

## ✨ Features

- **📚 Multi-content support**: Books, Movies, Series, Anime
- **🔗 Link or text**: Send URLs or just titles
- **🤖 AI-powered analysis**: Cloudflare Workers AI extracts metadata
- **💾 GitHub integration**: Saves to `<repository>/inbox/`
- **⏭️ Queue system**: Async processing via Cloudflare Queue
- **🔍 Deduplication**: Prevents duplicate entries
- **⌨️ Button-based UI**: All interactions via Telegram buttons

## 🏗 Architecture

```
Telegram → Cloudflare Worker
  ├── Cloudflare KV (state + dedup)
  ├── Cloudflare AI (text LLM)
  ├── Cloudflare Queue (async processing)
  └── GitHub API → <repository>/inbox/
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
    ├── github.rs       # GitHub API (commit to inbox/)
    ├── ai.rs           # Cloudflare AI analysis
    ├── parser.rs       # URL/text parsing
    ├── state.rs        # UserState, PendingItem, dedup
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
# Production
npx wrangler kv namespace create STATE_STORE
npx wrangler kv namespace create DEDUP_STORE

# Preview (for local testing)
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
# Telegram
npx wrangler secret put BOT_TOKEN
npx wrangler secret put ALLOWED_USERNAME

# GitHub
npx wrangler secret put GITHUB_TOKEN
npx wrangler secret put GITHUB_REPO

# Cloudflare AI (optional, has defaults)
npx wrangler secret put CLOUDFLARE_ACCOUNT_ID
npx wrangler secret put CLOUDFLARE_API_TOKEN
npx wrangler secret put AI_MODEL
# Default: @cf/meta/llama-3.2-11b-instruct
```

### 5. Deploy

```bash
npx wrangler deploy
```

### 6. Set Telegram webhook

```bash
curl -F "url=https://<YOUR_WORKER_URL>/webhook" https://api.telegram.org/bot<YOUR_BOT_TOKEN>/setWebhook
```

## 📖 Usage

### Send a link

```
User: https://www.goodreads.com/book/show/123
Bot: ⏳ Processing link...
Bot: ✅ Saved: inbox/books/read/2026-07-06_book-title.md
```

### Send a title

```
User: Lord of the Rings
Bot: What type is this?
     [📚 Book] [🎬 Movie]
     [📺 Series] [🎌 Anime]
     [📋 Other] [❌ Cancel]

User: [📚 Book]
Bot: 📚 Book — already consumed or in watchlist?
     [✅ Done] [📋 To-read]
     [❌ Cancel]

User: [📋 To-read]
Bot: Add details? (author, year) or skip:
     [⏭ Skip] [❌ Cancel]

User: [⏭ Skip]
Bot: ⏳ Saving...
Bot: ✅ Saved: inbox/books/to-read/2026-07-06_lord-of-the-rings.md
```

## 📁 Inbox Structure

```
inbox/
├── books/
│   ├── read/
│   └── to-read/
├── movies/
│   ├── watched/
│   └── to-watch/
├── series/
│   ├── watched/
│   └── to-watch/
├── anime/
│   ├── watched/
│   └── to-watch/
└── watchlist/
```

## 🔒 Security

- Only the allowed Telegram username can use the bot
- All secrets stored in Cloudflare Secrets
- No hardcoded credentials
- KV-based deduplication

## 📄 License

MIT