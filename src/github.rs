use crate::get_env_or_secret;
use crate::parser::ParserService;
use crate::state::PendingItem;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use std::time::Duration;
use worker::*;

/// Service for interacting with the GitHub Repository Contents + Git Data APIs.
/// Handles committing binary assets, pending YAML items, reply files, and logs
/// to a configured GitHub repository.
pub struct GitHubService;

impl GitHubService {
    /// Commits a binary file (photo/PDF) into inbox/assets/, returning the
    /// committed path. Unlike a Telegram file_id (which can expire), this is
    /// a permanent copy living in the private repo.
    ///
    /// The filename is derived from `item.id` (see
    /// `ParserService::generate_asset_filename`), which already includes
    /// second-level precision and the first 20 characters of the title with
    /// full Unicode support, so collisions are extremely unlikely. If a 409
    /// still occurs (e.g. the exact same file being saved twice), the error
    /// is propagated — silent overwrites are not acceptable.
    pub async fn save_asset(env: &Env, filename: &str, bytes: &[u8]) -> Result<String> {
        let token = env.secret("GITHUB_TOKEN")?.to_string();
        let repo = get_env_or_secret(env, "GITHUB_REPO", "Sc0rri/wiki");

        let path = format!("inbox/assets/{}", filename);
        let content_base64 = STANDARD.encode(bytes);

        Self::put_contents(&token, &repo, &path, &content_base64, None).await?;
        crate::log_event!("info", "github.asset.created", "path={}", path);
        Ok(path)
    }

    /// Saves a pending item to inbox/pending/ and appends the buffered log
    /// lines into the shared daily log file inbox/logs/<date>.log in a single
    /// atomic commit.
    ///
    /// Uses the Git Data API (create tree → create commit → update ref)
    /// instead of the Contents API, so both files land in one commit.
    /// This avoids the 409 race conditions that two separate Contents API
    /// calls would cause when the bot processes multiple messages concurrently.
    pub async fn save_to_inbox(
        env: &Env,
        item: &PendingItem,
        log_lines: &[String],
    ) -> Result<String> {
        let token = env.secret("GITHUB_TOKEN")?.to_string();
        let repo = get_env_or_secret(env, "GITHUB_REPO", "Sc0rri/wiki");

        let filename = ParserService::generate_filename(item);
        let pending_path = format!("inbox/pending/{}", filename);
        let pending_content = Self::generate_yaml(item);

        let date = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let log_path = format!("inbox/logs/{}.log", date);
        let log_content = Self::build_daily_log_content(&token, &repo, &date, log_lines).await?;

        Self::commit_files(
            &token,
            &repo,
            &format!("Pending: {} + logs [{}]", filename, date),
            &[(&pending_path, &pending_content), (&log_path, &log_content)],
        )
        .await?;

        crate::log_event!(
            "info",
            "github.commit.success",
            "pending={} log={} commit=git-data-api",
            pending_path,
            log_path
        );

        Ok(pending_path)
    }

    /// Flushes any remaining buffered log lines to inbox/logs/<date>.log
    /// without a pending item. Called at the end of handle_update to ensure
    /// logs from commands and other non-save paths are persisted.
    pub async fn flush_logs_only(env: &Env, log_lines: &[String]) {
        if log_lines.is_empty() {
            return;
        }
        let token = match env.secret("GITHUB_TOKEN") {
            Ok(t) => t.to_string(),
            Err(_) => return,
        };
        let repo = get_env_or_secret(env, "GITHUB_REPO", "Sc0rri/wiki");

        let date = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let log_path = format!("inbox/logs/{}.log", date);
        let log_content = match Self::build_daily_log_content(&token, &repo, &date, log_lines).await
        {
            Ok(content) => content,
            Err(e) => {
                crate::log_event!("warn", "github.flush_logs.build_failed", "error={:?}", e);
                return;
            }
        };

        match Self::commit_files(
            &token,
            &repo,
            &format!("Logs: {} entries [{}]", log_lines.len(), date),
            &[(&log_path, &log_content)],
        )
        .await
        {
            Ok(_sha) => {}
            Err(e) => {
                crate::logger::restore_logs(log_lines);
                // Fallback to Contents API if Git Data API fails.
                crate::log_event!("warn", "github.flush_logs.fallback", "error={:?}", e);
                if let Err(write_err) =
                    Self::put_text_file(&token, &repo, &log_path, &log_content).await
                {
                    crate::log_event!(
                        "warn",
                        "github.flush_logs.contents_failed",
                        "error={:?}",
                        write_err
                    );
                    return;
                }
                for line in log_lines {
                    // Extract level, name, message from the formatted line.
                    // Format: "2026-07-30T10:10:10.843Z [error] name - msg"
                    let level = line
                        .split('[')
                        .nth(1)
                        .and_then(|s| s.split(']').next())
                        .unwrap_or("info");
                    let name = line
                        .split("] ")
                        .nth(1)
                        .and_then(|s| s.split(" - ").next())
                        .unwrap_or("unknown");
                    let msg = line.split(" - ").nth(1).unwrap_or(line);
                    Self::append_log(env, level, name, msg).await;
                }
            }
        }
    }

    /// Writes a user's answer to a clarifying question the compiler asked,
    /// as inbox/pending/<id>.reply.yaml — same GitHub Contents API pattern
    /// as save_to_inbox, just a different, simpler payload.
    pub async fn save_reply_to_inbox(env: &Env, item_id: &str, reply_text: &str) -> Result<String> {
        let token = env.secret("GITHUB_TOKEN")?.to_string();
        let repo = get_env_or_secret(env, "GITHUB_REPO", "Sc0rri/wiki");

        let path = format!("inbox/pending/{}.reply.yaml", item_id);
        let now = chrono::Utc::now().to_rfc3339();
        let content = format!(
            "---\nreplies_to: {}\ntext: \"{}\"\ncreated: {}\n---\n",
            item_id,
            Self::yaml_quote(reply_text),
            now
        );
        let content_base64 = STANDARD.encode(&content);

        let url = format!("https://api.github.com/repos/{}/contents/{}", repo, path);

        let payload = serde_json::json!({
            "message": format!("Clarification reply for {}", item_id),
            "content": content_base64,
            "branch": "main"
        });

        let headers = Headers::new();
        headers.set(
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36",
        )?;
        headers.set("Accept-Language", "ru-RU,ru;q=0.9,en;q=0.8")?;
        headers.set("Accept-Encoding", "gzip, deflate, br")?;
        headers.set("Cache-Control", "no-cache")?;
        headers.set("Authorization", &format!("Bearer {}", token))?;
        headers.set("Content-Type", "application/json")?;

        let mut req_init = RequestInit::new();
        req_init.with_method(Method::Put);
        req_init.with_headers(headers);
        req_init.with_body(Some(serde_json::to_string(&payload)?.into()));

        let req = Request::new_with_init(&url, &req_init)?;
        let mut resp = Fetch::Request(req).send().await?;

        if resp.status_code() != 201 && resp.status_code() != 200 {
            let err_text = resp.text().await?;
            crate::log_event!(
                "error",
                "github.reply.failed",
                "status={} body={}",
                resp.status_code(),
                err_text
            );
            return Err(worker::Error::from(format!(
                "GitHub API error: {}",
                err_text
            )));
        }

        crate::log_event!("info", "github.reply.success", "path={}", path);

        Ok(path)
    }

    async fn build_daily_log_content(
        token: &str,
        repo: &str,
        date: &str,
        new_lines: &[String],
    ) -> Result<String> {
        let existing = Self::read_log_file(token, repo, date)
            .await
            .unwrap_or_default();
        Ok(Self::merge_log_content(&existing, new_lines))
    }

    fn merge_log_content(existing: &str, new_lines: &[String]) -> String {
        let mut merged = Vec::new();

        if !existing.trim().is_empty() {
            merged.extend(
                existing
                    .lines()
                    .filter(|line| !line.trim().is_empty())
                    .map(|line| line.to_string()),
            );
        }

        merged.extend(new_lines.iter().cloned());

        if merged.is_empty() {
            String::new()
        } else {
            format!("{}\n", merged.join("\n"))
        }
    }

    async fn read_log_file(token: &str, repo: &str, date: &str) -> Result<String> {
        let path = format!("inbox/logs/{}.log", date);
        let url = format!("https://api.github.com/repos/{}/contents/{}", repo, path);

        let headers = Headers::new();
        headers.set(
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36",
        )?;
        headers.set("Accept-Language", "ru-RU,ru;q=0.9,en;q=0.8")?;
        headers.set("Accept-Encoding", "gzip, deflate, br")?;
        headers.set("Cache-Control", "no-cache")?;
        headers.set("Authorization", &format!("Bearer {}", token))?;
        headers.set("Accept", "application/vnd.github+json")?;

        let mut req_init = RequestInit::new();
        req_init.with_method(Method::Get);
        req_init.with_headers(headers);

        let req = Request::new_with_init(&url, &req_init)?;
        let mut resp = Fetch::Request(req).send().await?;

        if resp.status_code() == 404 {
            return Ok(String::new());
        }
        if resp.status_code() != 200 {
            let body = resp.text().await?;
            return Err(worker::Error::from(format!(
                "GitHub GET {} failed: {}",
                path, body
            )));
        }

        let val: serde_json::Value = resp.json().await?;
        let content = val
            .get("content")
            .and_then(|c| c.as_str())
            .map(|c| c.replace('\n', ""))
            .unwrap_or_default();

        if content.is_empty() {
            return Ok(String::new());
        }

        let decoded = STANDARD
            .decode(content)
            .map_err(|e| worker::Error::from(format!("Base64 decode failed: {}", e)))?;
        String::from_utf8(decoded)
            .map_err(|e| worker::Error::from(format!("UTF-8 decode failed: {}", e)))
    }

    /// Appends a log line to inbox/logs/<date>.log in the GitHub repo.
    /// Uses GET (to fetch existing content) + PUT (to overwrite with appended
    /// line). Best-effort — only used as a fallback when the Git Data API
    /// commit fails (see `flush_logs_only`). The main logging path now uses
    /// the buffered log + Git Data API atomic commit.
    /// Retries up to 3 times on 409 conflicts.
    pub async fn append_log(env: &Env, level: &str, name: &str, message: &str) {
        let token = match env.secret("GITHUB_TOKEN") {
            Ok(t) => t.to_string(),
            Err(_) => return,
        };
        let repo = get_env_or_secret(env, "GITHUB_REPO", "Sc0rri/wiki");

        let date = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let path = format!("inbox/logs/{}.log", date);
        let url = format!("https://api.github.com/repos/{}/contents/{}", repo, path);

        let new_line = format!("{}\n", crate::logger::format_log_line(level, name, message));

        for attempt in 0..3 {
            // Try to GET existing file content (SHA + base64 body).
            let get_headers = Headers::new();
            let _ = get_headers.set(
                "User-Agent",
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36",
            );
            let _ = get_headers.set("Accept-Language", "ru-RU,ru;q=0.9,en;q=0.8");
            let _ = get_headers.set("Accept-Encoding", "gzip, deflate, br");
            let _ = get_headers.set("Cache-Control", "no-cache");
            let _ = get_headers.set("Authorization", &format!("Bearer {}", token));

            let mut get_req_init = RequestInit::new();
            get_req_init.with_method(Method::Get);
            get_req_init.with_headers(get_headers);

            let (existing_sha, existing_body) =
                if let Ok(req) = Request::new_with_init(&url, &get_req_init) {
                    if let Ok(mut resp) = Fetch::Request(req).send().await {
                        if resp.status_code() == 200 {
                            if let Ok(val) = resp.json::<serde_json::Value>().await {
                                let sha = val
                                    .get("sha")
                                    .and_then(|s| s.as_str())
                                    .map(|s| s.to_string());
                                let content = val
                                    .get("content")
                                    .and_then(|c| c.as_str())
                                    .map(|c| c.replace('\n', "").replace('\r', ""))
                                    .and_then(|c| {
                                        use base64::Engine;
                                        base64::engine::general_purpose::STANDARD.decode(c).ok()
                                    })
                                    .and_then(|bytes| String::from_utf8(bytes).ok())
                                    .unwrap_or_default();
                                (sha, content)
                            } else {
                                (None, String::new())
                            }
                        } else {
                            (None, String::new())
                        }
                    } else {
                        (None, String::new())
                    }
                } else {
                    (None, String::new())
                };

            let new_content = format!("{}{}\n", existing_body, new_line);
            let content_base64 = base64::engine::general_purpose::STANDARD.encode(&new_content);

            let mut payload = serde_json::json!({
                "message": format!("Log: {}", name),
                "content": content_base64,
                "branch": "main"
            });
            if let Some(sha) = existing_sha {
                payload
                    .as_object_mut()
                    .unwrap()
                    .insert("sha".to_string(), serde_json::Value::String(sha));
            }

            let put_headers = Headers::new();
            let _ = put_headers.set(
                "User-Agent",
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36",
            );
            let _ = put_headers.set("Accept-Language", "ru-RU,ru;q=0.9,en;q=0.8");
            let _ = put_headers.set("Accept-Encoding", "gzip, deflate, br");
            let _ = put_headers.set("Cache-Control", "no-cache");
            let _ = put_headers.set("Authorization", &format!("Bearer {}", token));
            let _ = put_headers.set("Content-Type", "application/json");

            let mut put_req_init = RequestInit::new();
            put_req_init.with_method(Method::Put);
            put_req_init.with_headers(put_headers);
            if let Ok(body) = serde_json::to_string(&payload) {
                put_req_init.with_body(Some(body.into()));
            }

            if let Ok(req) = Request::new_with_init(&url, &put_req_init) {
                if let Ok(resp) = Fetch::Request(req).send().await {
                    if resp.status_code() == 201 || resp.status_code() == 200 {
                        return; // Success
                    }
                    
                    // If 409 conflict and we have retries left, try again
                    if resp.status_code() == 409 && attempt < 2 {
                        let delay = (attempt + 1) * 100;
                        let _ = worker::Delay::from(Duration::from_millis(delay)).await;
                        continue;
                    }
                }
            }
            
            // If we get here, either non-409 error or final attempt failed
            return;
        }
    }

    // ── Contents API helpers ──────────────────────────────────────────────

    async fn put_text_file(token: &str, repo: &str, path: &str, content: &str) -> Result<()> {
        let content_base64 = STANDARD.encode(content);
        let url = format!("https://api.github.com/repos/{}/contents/{}", repo, path);

        let mut last_error = None;
        for attempt in 0..3 {
            let sha = Self::get_file_sha(token, repo, path).await.ok();
            
            let mut payload = serde_json::json!({
                "message": format!("Write {}", path),
                "content": content_base64.clone(),
                "branch": "main"
            });
            if let Some(ref sha) = sha {
                payload
                    .as_object_mut()
                    .unwrap()
                    .insert("sha".to_string(), serde_json::Value::String(sha.clone()));
            }

            let headers = Headers::new();
            headers.set(
                "User-Agent",
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36",
            )?;
            headers.set("Accept-Language", "ru-RU,ru;q=0.9,en;q=0.8")?;
            headers.set("Accept-Encoding", "gzip, deflate, br")?;
            headers.set("Cache-Control", "no-cache")?;
            headers.set("Authorization", &format!("Bearer {}", token))?;
            headers.set("Content-Type", "application/json")?;

            let mut req_init = RequestInit::new();
            req_init.with_method(Method::Put);
            req_init.with_headers(headers);
            req_init.with_body(Some(serde_json::to_string(&payload)?.into()));

            let req = Request::new_with_init(&url, &req_init)?;
            let mut resp = Fetch::Request(req).send().await?;

            if resp.status_code() == 201 || resp.status_code() == 200 {
                return Ok(());
            }

            let err_text = resp.text().await?;
            let err_str = format!("{}: {}", resp.status_code(), err_text);
            last_error = Some(err_str);

            // If it's a 409 conflict (SHA mismatch), retry after a short delay
            if resp.status_code() == 409 && attempt < 2 {
                crate::log_event!(
                    "warn",
                    "github.put_text_file.conflict",
                    "path={} attempt={} retrying...",
                    path,
                    attempt + 1
                );
                // Small delay to let concurrent operations complete
                let delay = (attempt + 1) * 100;
                let _ = worker::Delay::from(Duration::from_millis(delay)).await;
                continue;
            }

            // For other errors, don't retry
            break;
        }

        Err(worker::Error::from(format!(
            "GitHub Contents API error ({}): failed after 3 attempts",
            last_error.unwrap_or_default()
        )))
    }

    /// PUTs a file to the GitHub Contents API. If `sha` is Some, updates an
    /// existing file; if None, creates a new file.
    async fn put_contents(
        token: &str,
        repo: &str,
        path: &str,
        content_base64: &str,
        sha: Option<&str>,
    ) -> Result<()> {
        let url = format!("https://api.github.com/repos/{}/contents/{}", repo, path);

        let mut payload = serde_json::json!({
            "message": format!("Add asset: {}", path),
            "content": content_base64,
            "branch": "main"
        });
        if let Some(s) = sha {
            payload
                .as_object_mut()
                .unwrap()
                .insert("sha".to_string(), serde_json::Value::String(s.to_string()));
        }

        let headers = Headers::new();
        headers.set(
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36",
        )?;
        headers.set("Accept-Language", "ru-RU,ru;q=0.9,en;q=0.8")?;
        headers.set("Accept-Encoding", "gzip, deflate, br")?;
        headers.set("Cache-Control", "no-cache")?;
        headers.set("Authorization", &format!("Bearer {}", token))?;
        headers.set("Content-Type", "application/json")?;

        let mut req_init = RequestInit::new();
        req_init.with_method(Method::Put);
        req_init.with_headers(headers);
        req_init.with_body(Some(serde_json::to_string(&payload)?.into()));

        let req = Request::new_with_init(&url, &req_init)?;
        let mut resp = Fetch::Request(req).send().await?;

        if resp.status_code() != 201 && resp.status_code() != 200 {
            let err_text = resp.text().await?;
            return Err(worker::Error::from(format!(
                "GitHub Contents API error ({}): {}",
                resp.status_code(),
                err_text
            )));
        }

        Ok(())
    }

    /// Fetches the SHA of an existing file via the Contents API.
    async fn get_file_sha(token: &str, repo: &str, path: &str) -> Result<String> {
        let url = format!("https://api.github.com/repos/{}/contents/{}", repo, path);

        let headers = Headers::new();
        headers.set(
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36",
        )?;
        headers.set("Accept-Language", "ru-RU,ru;q=0.9,en;q=0.8")?;
        headers.set("Accept-Encoding", "gzip, deflate, br")?;
        headers.set("Cache-Control", "no-cache")?;
        headers.set("Authorization", &format!("Bearer {}", token))?;
        headers.set("Accept", "application/vnd.github+json")?;

        let mut req_init = RequestInit::new();
        req_init.with_method(Method::Get);
        req_init.with_headers(headers);

        let req = Request::new_with_init(&url, &req_init)?;
        let mut resp = Fetch::Request(req).send().await?;

        let status = resp.status_code();
        if status != 200 {
            let body = resp.text().await?;
            return Err(worker::Error::from(format!(
                "GitHub GET {} failed ({}): {}",
                path, status, body
            )));
        }

        let body: serde_json::Value = resp.json().await?;
        body["sha"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| worker::Error::from(format!("No SHA in response for {}", path)))
    }

    // ── Git Data API helpers ──────────────────────────────────────────────

    /// Creates a tree with the given files and commits it to `refs/heads/main`
    /// in a single atomic operation. Returns the new commit SHA.
    /// Retries up to 3 times on 422 errors (concurrent ref updates).
    async fn commit_files(
        token: &str,
        repo: &str,
        message: &str,
        files: &[(&str, &str)],
    ) -> Result<String> {
        let mut last_error = None;
        
        for attempt in 0..3 {
            // 1. Get the current HEAD commit SHA and tree SHA.
            let ref_url = format!("https://api.github.com/repos/{}/git/ref/heads/main", repo);
            let ref_resp = match Self::github_get(token, &ref_url).await {
                Ok(r) => r,
                Err(e) => {
                    last_error = Some(e);
                    break;
                }
            };
            let head_sha = ref_resp["object"]["sha"]
                .as_str()
                .ok_or_else(|| worker::Error::from("GitHub: no object.sha in ref response"))?
                .to_string();

            let commit_url = format!(
                "https://api.github.com/repos/{}/git/commits/{}",
                repo, head_sha
            );
            let commit_resp = Self::github_get(token, &commit_url).await?;
            let base_tree_sha = commit_resp["tree"]["sha"]
                .as_str()
                .ok_or_else(|| worker::Error::from("GitHub: no tree.sha in commit response"))?
                .to_string();

            // 2. Build tree entries.
            let mut tree_entries: Vec<serde_json::Value> = Vec::new();

            for (path, content) in files {
                let blob_sha = Self::create_blob(token, repo, content).await?;
                tree_entries.push(serde_json::json!({
                    "path": path,
                    "mode": "100644",
                    "type": "blob",
                    "sha": blob_sha,
                }));
            }

            // 3. Create a new tree with the base tree + our new entries.
            let tree_url = format!("https://api.github.com/repos/{}/git/trees", repo);
            let tree_payload = serde_json::json!({
                "base_tree": base_tree_sha,
                "tree": tree_entries,
            });
            let tree_resp = Self::github_post(token, &tree_url, &tree_payload).await?;
            let new_tree_sha = tree_resp["sha"]
                .as_str()
                .ok_or_else(|| worker::Error::from("GitHub: no sha in tree response"))?
                .to_string();

            // 4. Create a commit pointing to the new tree.
            let commit_url = format!("https://api.github.com/repos/{}/git/commits", repo);
            let commit_payload = serde_json::json!({
                "message": message,
                "tree": new_tree_sha,
                "parents": [head_sha],
            });
            let commit_resp = Self::github_post(token, &commit_url, &commit_payload).await?;
            let new_commit_sha = commit_resp["sha"]
                .as_str()
                .ok_or_else(|| worker::Error::from("GitHub: no sha in commit response"))?
                .to_string();

            // 5. Update the branch reference to point to the new commit.
            //    force=true is safe: this bot is the only writer to the repo.
            let ref_url = format!("https://api.github.com/repos/{}/git/refs/heads/main", repo);
            let ref_payload = serde_json::json!({
                "sha": new_commit_sha
            });
            match Self::github_patch(token, &ref_url, &ref_payload).await {
                Ok(_) => return Ok(new_commit_sha),
                Err(e) => {
                    let err_str = e.to_string();
                    // 422 = ref was updated by another commit → retry
                    if err_str.contains("422") && attempt < 2 {
                        crate::log_event!(
                            "warn",
                            "github.commit_files.conflict",
                            "attempt={} retrying...",
                            attempt + 1
                        );
                        let delay = (attempt + 1) * 200;
                        let _ = worker::Delay::from(Duration::from_millis(delay)).await;
                        continue;
                    }
                    last_error = Some(e);
                    break;
                }
            }
        }

        Err(last_error.unwrap_or_else(|| worker::Error::from("GitHub: commit_files failed after 3 attempts")))
    }

    /// Creates a Git blob and returns its SHA.
    async fn create_blob(token: &str, repo: &str, content: &str) -> Result<String> {
        let url = format!("https://api.github.com/repos/{}/git/blobs", repo);
        let payload = serde_json::json!({
            "content": content,
            "encoding": "utf-8",
        });
        let resp = Self::github_post(token, &url, &payload).await?;
        resp["sha"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| worker::Error::from("GitHub: no sha in blob response"))
    }

    // ── Low-level HTTP helpers ────────────────────────────────────────────

    async fn github_get(token: &str, url: &str) -> Result<serde_json::Value> {
        let headers = Headers::new();
        headers.set(
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36",
        )?;
        headers.set("Accept-Language", "ru-RU,ru;q=0.9,en;q=0.8")?;
        headers.set("Accept-Encoding", "gzip, deflate, br")?;
        headers.set("Cache-Control", "no-cache")?;
        headers.set("Authorization", &format!("Bearer {}", token))?;
        headers.set("Accept", "application/vnd.github+json")?;

        let mut req_init = RequestInit::new();
        req_init.with_method(Method::Get);
        req_init.with_headers(headers);

        let req = Request::new_with_init(url, &req_init)?;
        let mut resp = Fetch::Request(req).send().await?;

        let status = resp.status_code();
        let body = resp.text().await?;

        if status < 200 || status >= 300 {
            crate::log_event!(
                "error",
                "github.api.get_failed",
                "status={} url={} body={}",
                status,
                url,
                body
            );
            return Err(worker::Error::from(format!(
                "GitHub GET {} failed: {}",
                url, body
            )));
        }

        serde_json::from_str(&body)
            .map_err(|e| worker::Error::from(format!("GitHub GET {} JSON parse error: {}", url, e)))
    }

    async fn github_post(
        token: &str,
        url: &str,
        payload: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let headers = Headers::new();
        headers.set(
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36",
        )?;
        headers.set("Accept-Language", "ru-RU,ru;q=0.9,en;q=0.8")?;
        headers.set("Accept-Encoding", "gzip, deflate, br")?;
        headers.set("Cache-Control", "no-cache")?;
        headers.set("Authorization", &format!("Bearer {}", token))?;
        headers.set("Content-Type", "application/json")?;
        headers.set("Accept", "application/vnd.github+json")?;

        let mut req_init = RequestInit::new();
        req_init.with_method(Method::Post);
        req_init.with_headers(headers);
        req_init.with_body(Some(serde_json::to_string(payload)?.into()));

        let req = Request::new_with_init(url, &req_init)?;
        let mut resp = Fetch::Request(req).send().await?;

        let status = resp.status_code();
        let body = resp.text().await?;

        if status < 200 || status >= 300 {
            crate::log_event!(
                "error",
                "github.api.post_failed",
                "status={} url={} body={}",
                status,
                url,
                body
            );
            return Err(worker::Error::from(format!(
                "GitHub POST {} failed: {}",
                url, body
            )));
        }

        serde_json::from_str(&body).map_err(|e| {
            worker::Error::from(format!("GitHub POST {} JSON parse error: {}", url, e))
        })
    }

    async fn github_patch(
        token: &str,
        url: &str,
        payload: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let headers = Headers::new();
        headers.set(
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36",
        )?;
        headers.set("Accept-Language", "ru-RU,ru;q=0.9,en;q=0.8")?;
        headers.set("Accept-Encoding", "gzip, deflate, br")?;
        headers.set("Cache-Control", "no-cache")?;
        headers.set("Authorization", &format!("Bearer {}", token))?;
        headers.set("Content-Type", "application/json")?;
        headers.set("Accept", "application/vnd.github+json")?;

        let mut req_init = RequestInit::new();
        req_init.with_method(Method::Patch);
        req_init.with_headers(headers);
        req_init.with_body(Some(serde_json::to_string(payload)?.into()));

        let req = Request::new_with_init(url, &req_init)?;
        let mut resp = Fetch::Request(req).send().await?;

        let status = resp.status_code();
        let body = resp.text().await?;

        if status < 200 || status >= 300 {
            crate::log_event!(
                "error",
                "github.api.patch_failed",
                "status={} url={} body={}",
                status,
                url,
                body
            );
            return Err(worker::Error::from(format!(
                "GitHub PATCH {} failed: {}",
                url, body
            )));
        }

        serde_json::from_str(&body).map_err(|e| {
            worker::Error::from(format!("GitHub PATCH {} JSON parse error: {}", url, e))
        })
    }

    /// Escapes a string for safe inclusion in a YAML double-quoted value.
    /// Handles backslash, double-quote, carriage return, newline, and tab so
    /// user-provided text can never break out of the quoted scalar.
    fn yaml_quote(s: &str) -> String {
        s.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\r', "\\r")
            .replace('\n', "\\n")
            .replace('\t', "\\t")
    }

    /// Serializes a pending item into YAML frontmatter.
    /// All string fields are escaped via `yaml_quote` to prevent YAML injection
    /// or parse errors, and the output always ends with a trailing `---` separator.
    fn generate_yaml(item: &PendingItem) -> String {
        let mut yaml = String::new();

        yaml.push_str("---\n");
        yaml.push_str(&format!("id: {}\n", item.id));
        yaml.push_str(&format!("created: {}\n", item.created));
        yaml.push_str(&format!("source: {}\n", item.source));
        yaml.push_str(&format!(
            "provider: {}\n",
            item.provider.label().to_lowercase()
        ));
        yaml.push_str(&format!("chat_id: {}\n", item.chat_id));

        if let Some(ref url) = item.url {
            yaml.push_str(&format!("url: \"{}\"\n", Self::yaml_quote(url)));
        }

        yaml.push_str(&format!(
            "type: {}\n",
            item.knowledge_type.label().to_lowercase()
        ));
        yaml.push_str(&format!(
            "status: {}\n",
            item.status.label(&item.knowledge_type).to_lowercase()
        ));
        yaml.push_str(&format!("title: \"{}\"\n", Self::yaml_quote(&item.title)));

        if let Some(ref raw) = item.raw_text {
            if raw != &item.title {
                yaml.push_str(&format!("raw_text: \"{}\"\n", Self::yaml_quote(raw)));
            }
        }

        if let Some(ref author) = item.author {
            yaml.push_str(&format!("author: \"{}\"\n", Self::yaml_quote(author)));
        }

        if let Some(ref language) = item.language {
            yaml.push_str(&format!("language: {}\n", language));
        }

        if let Some(year) = item.year {
            yaml.push_str(&format!("year: {}\n", year));
        }

        if let Some(season) = item.season {
            yaml.push_str(&format!("season: {}\n", season));
        }

        if let Some(stars) = item.stars {
            yaml.push_str(&format!("stars: {}\n", stars));
        }

        if let Some(rating) = item.rating {
            yaml.push_str(&format!("rating: {}\n", rating));
        }

        if let Some(ref comment) = item.comment {
            yaml.push_str(&format!("comment: \"{}\"\n", Self::yaml_quote(comment)));
        }

        if !item.tags.is_empty() {
            yaml.push_str("tags:\n");
            for tag in &item.tags {
                yaml.push_str(&format!("  - \"{}\"\n", Self::yaml_quote(tag)));
            }
        } else {
            yaml.push_str("tags: []\n");
        }

        if let Some(ref mime) = item.asset_mime {
            yaml.push_str(&format!("asset_mime: {}\n", mime));
        }
        if let (Some(w), Some(h)) = (item.asset_width, item.asset_height) {
            yaml.push_str(&format!("asset_width: {}\n", w));
            yaml.push_str(&format!("asset_height: {}\n", h));
        }
        if let Some(ref sha) = item.asset_sha256 {
            yaml.push_str(&format!("asset_sha256: {}\n", sha));
        }

        yaml.push_str("---\n");

        yaml
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{ContentStatus, KnowledgeType, ResourceProvider};

    #[test]
    fn generate_yaml_should_create_valid_frontmatter() {
        let mut item = PendingItem::new("Test Article".to_string(), KnowledgeType::Link, 12345);
        item.author = Some("Test Author".to_string());
        item.year = Some(2024);
        item.status = ContentStatus::Backlog;
        item.provider = ResourceProvider::Web;
        item.tags = vec!["rust".to_string(), "wasm".to_string()];

        let yaml = GitHubService::generate_yaml(&item);

        assert!(yaml.contains("type: link"));
        assert!(yaml.contains("title: \"Test Article\""));
        assert!(yaml.contains("author: \"Test Author\""));
        assert!(yaml.contains("year: 2024"));
        assert!(yaml.contains("status: backlog"));
        assert!(yaml.contains("source: telegram"));
        assert!(yaml.contains("provider: web"));
        assert!(yaml.contains("chat_id: 12345"));
        assert!(yaml.contains("tags:"));
        assert!(yaml.contains("- \"rust\""));
        assert!(yaml.contains("id: "));
        assert!(yaml.contains("created: "));
        assert!(yaml.ends_with("---\n"));
    }

    #[test]
    fn generate_yaml_should_have_empty_tags_array() {
        let item = PendingItem::new("No Tags".to_string(), KnowledgeType::Book, 12345);
        let yaml = GitHubService::generate_yaml(&item);
        assert!(yaml.contains("tags: []\n"));
    }
}