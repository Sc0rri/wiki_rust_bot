use worker::*;

/// Core modules for the Wiki Rust Bot Cloudflare Worker.
///
/// # Module Organization
/// - `app` - Webhook handler and state machine
/// - `dedup` - KV-based deduplication service
/// - `detector` - URL provider detection
/// - `github` - GitHub API integration
/// - `logger` - Logging utilities and event buffering
/// - `parser` - Text processing (slugify, URL detection)
/// - `resolver` - External metadata resolvers (GitHub, YouTube, HTML)
/// - `state` - User state management and knowledge types
/// - `telegram` - Telegram API types and service
mod app;
mod dedup;
mod detector;
mod github;
mod logger;
mod parser;
mod resolver;
mod state;
mod telegram;

/// Retrieves an environment variable or secret, falling back to a default.
/// Tries `env.secret(name)` first (secure Cloudflare secret storage), then
/// `env.var(name)` (regular environment variable), and finally returns
/// `default` if neither is set.
pub(crate) fn get_env_or_secret(env: &Env, name: &str, default: &str) -> String {
    env.secret(name)
        .map(|v| v.to_string())
        .or_else(|_| env.var(name).map(|v| v.to_string()))
        .unwrap_or_else(|_| default.to_string())
}

/// Fetches incoming Telegram webhook updates and routes them to the app handler.
///
/// Supported update types:
/// - GET /webhook or empty path: returns bot status message
/// - POST /webhook: processes the Telegram update JSON
/// - All other methods/paths: returns 404 Not Found
#[event(fetch)]
async fn fetch(req: HttpRequest, env: Env, ctx: Context) -> Result<HttpResponse> {
    let mut req = match worker::Request::try_from(req) {
        Ok(r) => r,
        Err(e) => {
            log_event!("warn", "http.request.conversion_failed", "error={:?}", e);
            let err_res = Response::error("Bad Request", 400)?;
            return err_res.try_into();
        }
    };

    let path = req.path();
    let path_clean = path.trim_end_matches('/');
    let method = req.method().to_string();

    if method == "GET" && (path_clean == "/webhook" || path_clean.is_empty()) {
        let res = Response::ok(
            "🤖 Wiki Bot is running! Please send POST requests via Telegram webhooks.",
        )?;
        return res.try_into();
    }

    if method != "POST" || path_clean != "/webhook" {
        let err_res = Response::error("Not Found", 404)?;
        return err_res.try_into();
    }

    let update_raw = req.text().await?;
    log_event!(
        "info",
        "telegram.webhook.received",
        "path={} bytes={}",
        path_clean,
        update_raw.len()
    );
    app::handle_update(env, ctx, update_raw).await?;

    let res = Response::empty()?;
    res.try_into()
}
