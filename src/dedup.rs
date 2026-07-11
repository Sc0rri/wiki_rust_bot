use worker::*;

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
        format!("url:{}", url)
    }

    pub fn title_key(title: &str) -> String {
        format!("title:{}", title.to_lowercase())
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