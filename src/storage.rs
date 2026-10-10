//! App data in `~/.venus/`: session state, recent requests, history.
//!
//! Set `VENUS_HOME` to use another directory (tests, or running a dev
//! build without touching your real data).

use crate::model::{AppState, HistoryEntry, HistorySummary, RecentRequest};
use serde::{de::DeserializeOwned, Serialize};
use std::fs;
use std::path::PathBuf;

pub const MAX_HISTORY: usize = 50;
pub const MAX_RECENT: usize = 50;
const HISTORY_TTL_SECS: f64 = 7.0 * 24.0 * 60.0 * 60.0;

/// Seconds since the Unix epoch (what all stored timestamps use).
pub fn now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

pub(crate) fn dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("VENUS_HOME").or_else(|| std::env::var_os("MERCURY_HOME")) {
        return PathBuf::from(dir);
    }
    dirs::home_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join(".venus")
}

pub(crate) fn read(name: &str) -> std::io::Result<Vec<u8>> {
    let legacy =
        if std::env::var_os("VENUS_HOME").is_none() && std::env::var_os("MERCURY_HOME").is_none() {
            dirs::home_dir().map(|home| home.join(".mercury"))
        } else {
            None
        };
    read_with_legacy(&dir(), legacy.as_deref(), name)
}

fn read_with_legacy(
    directory: &std::path::Path,
    legacy: Option<&std::path::Path>,
    name: &str,
) -> std::io::Result<Vec<u8>> {
    let destination = directory.join(name);
    match fs::read(&destination) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let Some(legacy) = legacy else {
                return Err(error);
            };
            let bytes = fs::read(legacy.join(name))?;
            let parent = destination
                .parent()
                .ok_or_else(|| std::io::Error::other("Invalid data path"))?;
            fs::create_dir_all(parent)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(directory, fs::Permissions::from_mode(0o700))?;
                fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
            }
            crate::insomnia_workspace::atomic_write(&destination, &bytes)
                .map_err(std::io::Error::other)?;
            Ok(bytes)
        }
        result => result,
    }
}

fn load<T: DeserializeOwned>(name: &str) -> Option<T> {
    serde_json::from_slice(&read(name).ok()?).ok()
}

fn save<T: Serialize + ?Sized>(name: &str, value: &T) {
    let dir = dir();
    let result = fs::create_dir_all(&dir).and_then(|_| {
        let json = serde_json::to_string_pretty(value).expect("app data serializes");
        fs::write(dir.join(name), json)
    });
    if let Err(e) = result {
        eprintln!("Failed to save {name}: {e}");
    }
}

pub fn load_state() -> Option<AppState> {
    load("state.json")
}

pub fn save_state(state: &AppState) {
    save("state.json", state);
}

pub fn load_recent() -> Vec<RecentRequest> {
    load("recent.json").unwrap_or_default()
}

pub fn save_recent(requests: &[RecentRequest]) {
    let skip = requests.len().saturating_sub(MAX_RECENT);
    save("recent.json", &requests[skip..]);
}

fn load_history() -> Vec<HistoryEntry> {
    load("history.json").unwrap_or_default()
}

/// Drop expired entries and keep the newest MAX_HISTORY (oldest first).
fn prune(entries: Vec<HistoryEntry>, now: f64) -> Vec<HistoryEntry> {
    let mut kept: Vec<_> = entries
        .into_iter()
        .filter(|e| e.timestamp > now - HISTORY_TTL_SECS)
        .collect();
    let excess = kept.len().saturating_sub(MAX_HISTORY);
    kept.drain(..excess);
    kept
}

pub fn load_history_summaries() -> Vec<HistorySummary> {
    prune(load_history(), now())
        .iter()
        .map(HistorySummary::from)
        .collect()
}

/// Full entry (with response body) for a summary the user clicked.
pub fn load_history_entry(timestamp: f64) -> Option<HistoryEntry> {
    load_history()
        .into_iter()
        .find(|e| e.timestamp == timestamp)
}

pub fn append_history(entry: HistoryEntry) {
    let mut entries = load_history();
    entries.push(entry);
    save("history.json", &prune(entries, now()));
}

pub fn clear_history() {
    let _ = fs::remove_file(dir().join("history.json"));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{HttpResponse, ResponseType};
    use crate::model::{HttpMethod, Request};

    #[test]
    fn legacy_profile_is_copied_without_overwriting_venus_or_touching_mercury() {
        let parent = tempfile::tempdir().unwrap();
        let legacy = parent.path().join(".mercury");
        let venus = parent.path().join(".venus");
        fs::create_dir_all(legacy.join("insomnia")).unwrap();
        for name in [
            "state.json",
            "recent.json",
            "history.json",
            "insomnia/session.json",
        ] {
            fs::write(legacy.join(name), b"{\"example\":true}").unwrap();
            assert_eq!(
                read_with_legacy(&venus, Some(&legacy), name).unwrap(),
                b"{\"example\":true}"
            );
            assert_eq!(fs::read(legacy.join(name)).unwrap(), b"{\"example\":true}");
        }
        fs::write(venus.join("insomnia/session.json"), b"null").unwrap();
        assert_eq!(
            read_with_legacy(&venus, Some(&legacy), "insomnia/session.json").unwrap(),
            b"null"
        );
        let isolated = parent.path().join("isolated");
        assert!(read_with_legacy(&isolated, None, "state.json").is_err());
        assert!(!isolated.exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(venus.join("history.json"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
            assert_eq!(
                fs::metadata(venus.join("insomnia"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
        }
    }

    fn entry(timestamp: f64) -> HistoryEntry {
        HistoryEntry {
            timestamp,
            request: Request {
                method: HttpMethod::POST,
                url: "https://a.com".into(),
                headers: "A: 1".into(),
                body: "{}".into(),
                ..Default::default()
            },
            response: HttpResponse {
                status: 201,
                status_text: "201 Created".into(),
                headers: vec![],
                cookies: vec![],
                body: "{\"id\":1}".into(),
                raw_bytes: None,
                duration_ms: 12,
                size_bytes: 8,
                content_type: "application/json".into(),
                response_type: ResponseType::Json,
            },
        }
    }

    #[test]
    fn prune_drops_expired_and_keeps_newest() {
        let now = 1_000_000_000.0;
        let mut entries = vec![entry(now - HISTORY_TTL_SECS - 1.0)];
        entries.extend((0..MAX_HISTORY + 5).map(|i| entry(now - 100.0 + i as f64)));
        let kept = prune(entries, now);
        assert_eq!(kept.len(), MAX_HISTORY);
        assert_eq!(kept[0].timestamp, now - 100.0 + 5.0);
    }

    #[test]
    fn reads_v0_2_history_format() {
        // entries written before HttpResponse was stored directly (no cookies field)
        let old = r#"[{"timestamp":1702400000.0,
            "request":{"method":"GET","url":"u","headers":"","body":""},
            "response":{"status":204,"status_text":"No Content","headers":[],"body":"",
              "content_type":"","response_type":"Empty","size_bytes":0,"duration_ms":50}}]"#;
        let entries: Vec<HistoryEntry> = serde_json::from_str(old).unwrap();
        assert_eq!(entries[0].response.response_type, ResponseType::Empty);
        assert!(entries[0].response.cookies.is_empty());
    }

    #[test]
    fn history_entry_roundtrips() {
        let json = serde_json::to_string(&entry(1.0)).unwrap();
        let back: HistoryEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(back.request, entry(1.0).request);
        assert_eq!(back.response.status, 201);
    }
}
