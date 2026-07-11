use worker::*;

/// Cloudflare KV keys are capped at 512 bytes. Leave headroom for the
/// "title:"/"url:" prefix (6 bytes) plus a safety margin.
const MAX_KEY_CONTENT_BYTES: usize = 500;

pub struct DedupService;

impl DedupService {
    pub async fn is_processed(kv: &kv::KvStore, key: &str) -> Result<bool> {
        match kv.get(key).text().await {
            Ok(Some(_)) => Ok(true),
            Ok(None) => Ok(false),
            Err(e) => {
                crate::log_event!("error", "dedup.check.failed", "error={:?}", e);
                Ok(false)
            }
        }
    }

    pub async fn mark_processed(kv: &kv::KvStore, key: &str) -> Result<()> {
        match kv.put(key, "1")?.execute().await {
            Ok(_) => {
                crate::log_event!("info", "dedup.marked", "key={}", key);
                Ok(())
            }
            Err(e) => {
                crate::log_event!("error", "dedup.mark.failed", "error={:?}", e);
                Err(worker::Error::from(e))
            }
        }
    }

    pub fn url_key(url: &str) -> String {
        format!("url:{}", Self::truncate_to_byte_limit(url, MAX_KEY_CONTENT_BYTES))
    }

    /// KV keys are capped at 512 bytes (UTF-8 encoded) by Cloudflare. A title
    /// can be an entire forwarded paragraph (e.g. a long Note), so it must be
    /// truncated to fit — otherwise the PUT fails outright ("KV PUT failed:
    /// 414 ... exceeds key length limit of 512") and dedup silently never
    /// registers for that item.
    pub fn title_key(title: &str) -> String {
        format!("title:{}", Self::truncate_to_byte_limit(&title.to_lowercase(), MAX_KEY_CONTENT_BYTES))
    }

    /// Truncates to at most `max_bytes` UTF-8 bytes, backing off to the
    /// nearest character boundary so multi-byte characters (Cyrillic, etc.)
    /// aren't split mid-encoding.
    fn truncate_to_byte_limit(s: &str, max_bytes: usize) -> &str {
        if s.len() <= max_bytes {
            return s;
        }
        let mut end = max_bytes;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        &s[..end]
    }

    /// Deletes every dedup entry (both "title:" and "url:" prefixes),
    /// paginating through the KV list since a namespace can hold more than
    /// one page (1000 keys) of entries. Returns the number of keys deleted.
    pub async fn clear_all(kv: &kv::KvStore) -> Result<usize> {
        let mut deleted = 0usize;

        for prefix in ["title:", "url:"] {
            let mut cursor: Option<String> = None;
            loop {
                let mut builder = kv.list().prefix(prefix.to_string());
                if let Some(c) = cursor.take() {
                    builder = builder.cursor(c);
                }
                let page = builder
                    .execute()
                    .await
                    .map_err(|e| worker::Error::from(e.to_string()))?;

                for key in &page.keys {
                    match kv.delete(&key.name).await {
                        Ok(_) => deleted += 1,
                        Err(e) => {
                            crate::log_event!("warn", "dedup.clear.delete_failed", "key={} error={:?}", key.name, e);
                        }
                    }
                }

                if page.list_complete {
                    break;
                }
                match page.cursor {
                    Some(c) if !c.is_empty() => cursor = Some(c),
                    _ => break,
                }
            }
        }

        Ok(deleted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_key_should_stay_under_kv_key_limit() {
        let long_title = "а".repeat(1000); // Cyrillic: 2 bytes per char in UTF-8
        let key = DedupService::title_key(&long_title);
        assert!(key.len() <= 512, "key was {} bytes", key.len());
    }

    #[test]
    fn title_key_should_not_panic_on_multibyte_boundary() {
        // Odd-length repeats of a 2-byte char stress-test the char-boundary backoff.
        let title = "я".repeat(251); // 502 bytes, right at the edge of the cap
        let key = DedupService::title_key(&title);
        assert!(key.len() <= 512);
    }

    #[test]
    fn url_key_should_stay_under_kv_key_limit() {
        let long_url = format!("https://example.com/{}", "x".repeat(600));
        let key = DedupService::url_key(&long_url);
        assert!(key.len() <= 512, "key was {} bytes", key.len());
    }
}