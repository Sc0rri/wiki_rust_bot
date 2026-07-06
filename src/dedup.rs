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
}