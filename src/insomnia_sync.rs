//! Local Insomnia Cloud transport. Cloud branches are independent of Git branches.

use crate::insomnia_crypto as crypto;
use crate::insomnia_workspace::{self as workspace, Baseline};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::Path;
use std::time::Duration;

const MASTER: &str = "master";
const MAX_RESPONSE_BYTES: u64 = 32 * 1024 * 1024;

pub struct Cloud {
    client: Client,
    session: Value,
    project_id: String,
    #[cfg(test)]
    endpoint: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Branch {
    pub name: String,
    pub snapshots: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Entry {
    pub key: String,
    pub blob: String,
    pub name: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Snapshot {
    pub id: String,
    pub state: Vec<Entry>,
}

#[derive(Serialize, Deserialize)]
struct PendingPush {
    project_id: String,
    snapshot: Value,
    documents: BTreeMap<String, Value>,
    baseline: Baseline,
}

const PENDING: &str = ".insomnia-sync/pending-push.json";

fn read_session(content: &str) -> Result<Value, String> {
    let mut documents = HashMap::new();
    for line in content.lines().filter(|line| !line.trim().is_empty()) {
        let document: Value = serde_json::from_str(line)
            .map_err(|_| "Insomnia's local session database is invalid".to_string())?;
        let Some(id) = document["_id"].as_str() else {
            continue;
        };
        if document["$$deleted"] == true {
            documents.remove(id);
        } else {
            documents.insert(id.to_string(), document);
        }
    }
    let sessions: Vec<_> = documents
        .values()
        .filter(|document| document["id"].as_str().is_some_and(|id| !id.is_empty()))
        .collect();
    if sessions.len() != 1 {
        return Err("Sign in to Insomnia with one active account before syncing".into());
    }
    Ok(sessions[0].clone())
}

impl Cloud {
    pub fn from_local_session(project_id: &str, directory: &Path) -> Result<Self, String> {
        if !project_id.starts_with("prj_")
            || project_id.len() <= 4
            || !project_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        {
            return Err("Invalid Insomnia cloud project ID".into());
        }
        let content =
            fs::read_to_string(directory.join("insomnia.UserSession.db")).map_err(|_| {
                "Cannot read Insomnia's local login; open Insomnia and sign in".to_string()
            })?;
        let session = read_session(&content)?;
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| format!("Cannot initialize cloud client: {error}"))?;
        Ok(Self {
            client,
            session,
            project_id: project_id.into(),
            #[cfg(test)]
            endpoint: None,
        })
    }

    fn query(&self, query: &str, variables: Value) -> Result<Value, String> {
        let endpoint = "https://api.insomnia.rest/graphql?mercury";
        #[cfg(test)]
        let endpoint = self.endpoint.as_deref().unwrap_or(endpoint);
        let response = self
            .client
            .post(endpoint)
            .header("X-Session-Id", crypto::string(&self.session, "id")?)
            .header("X-Insomnia-Client", "production::linux::13.3.0")
            .header("X-Origin", "https://api.insomnia.rest")
            .json(&json!({"query": query, "variables": variables}))
            .send()
            .map_err(|_| "Cannot reach Insomnia Cloud".to_string())?;
        if !response.status().is_success() {
            return Err(format!(
                "Insomnia Cloud returned HTTP {}",
                response.status()
            ));
        }
        let mut content = Vec::new();
        response
            .take(MAX_RESPONSE_BYTES + 1)
            .read_to_end(&mut content)
            .map_err(|_| "Cannot read Insomnia Cloud response".to_string())?;
        if content.len() as u64 > MAX_RESPONSE_BYTES {
            return Err("Insomnia Cloud response exceeds 32 MiB".into());
        }
        let result: Value = serde_json::from_slice(&content)
            .map_err(|_| "Insomnia Cloud returned invalid JSON".to_string())?;
        if result["errors"]
            .as_array()
            .is_some_and(|errors| !errors.is_empty())
            || !result["data"].is_object()
        {
            return Err("Insomnia Cloud rejected the query; check login and project access".into());
        }
        Ok(result["data"].clone())
    }

    pub fn read_master_manifest(&self) -> Result<(Branch, Snapshot), String> {
        let data = self.query(
            "query ($projectId: ID!, $branch: String!) { branch(project: $projectId, name: $branch) { name snapshots } }",
            json!({"projectId": self.project_id, "branch": MASTER}),
        )?;
        let branch: Branch = serde_json::from_value(data["branch"].clone())
            .map_err(|_| "Remote master is missing or invalid".to_string())?;
        let head = validate_master(&branch)?;
        let data = self.query(
            "query ($projectId: ID!, $ids: [ID!]!) { snapshots(project: $projectId, ids: $ids) { id state { key blob name } } }",
            json!({"projectId": self.project_id, "ids": [head]}),
        )?;
        let mut snapshots: Vec<Snapshot> = serde_json::from_value(data["snapshots"].clone())
            .map_err(|_| "Remote master returned invalid snapshots".to_string())?;
        if snapshots.len() != 1 || snapshots[0].id != head {
            return Err("Remote master returned an incomplete snapshot".into());
        }
        let snapshot = snapshots.remove(0);
        if snapshot
            .state
            .iter()
            .any(|entry| entry.key.is_empty() || entry.blob.is_empty())
        {
            return Err("Remote master returned invalid resource IDs".into());
        }
        Ok((branch, snapshot))
    }

    fn project_key(&self) -> Result<Vec<u8>, String> {
        let data = self.query(
            "query ($projectId: ID!) { projectKey(projectId: $projectId) { encSymmetricKey } }",
            json!({"projectId": self.project_id}),
        )?;
        crypto::project_key(
            &self.session,
            crypto::string(&data["projectKey"], "encSymmetricKey")?,
        )
    }

    pub fn download_master(&self) -> Result<(Snapshot, BTreeMap<String, Value>), String> {
        let (_, snapshot) = self.read_master_manifest()?;
        let key = self.project_key()?;
        let ids: Vec<_> = snapshot
            .state
            .iter()
            .map(|entry| entry.blob.as_str())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        let mut blobs = HashMap::new();
        for batch in ids.chunks(50) {
            let data = self.query(
                "query ($projectId: ID!, $ids: [ID!]!) { blobs(project: $projectId, ids: $ids) { id content } }",
                json!({"projectId": self.project_id, "ids": batch}),
            )?;
            let downloaded = data["blobs"]
                .as_array()
                .ok_or_else(|| "Cloud returned invalid blobs".to_string())?;
            if downloaded.len() != batch.len() {
                return Err("Cloud returned missing blobs".into());
            }
            for blob in downloaded {
                let id = crypto::string(blob, "id")?;
                if !batch.contains(&id) || blobs.contains_key(id) {
                    return Err("Cloud returned unexpected blobs".into());
                }
                let encrypted: Value = serde_json::from_str(crypto::string(blob, "content")?)
                    .map_err(|_| "Cloud returned invalid encrypted blobs".to_string())?;
                blobs.insert(
                    id.to_string(),
                    crypto::decode_document(&key, id, &encrypted)?,
                );
            }
        }
        let mut documents = BTreeMap::new();
        for entry in &snapshot.state {
            let document = blobs
                .get(&entry.blob)
                .ok_or_else(|| "Missing cloud resource".to_string())?;
            if document["_id"] != entry.key || documents.contains_key(&entry.key) {
                return Err("Cloud resource identity does not match its snapshot".into());
            }
            documents.insert(entry.key.clone(), document.clone());
        }
        Ok((snapshot, documents))
    }

    fn publish_pending(&self, root: &Path, pending: PendingPush) -> Result<String, String> {
        if pending.project_id != self.project_id {
            return Err("Pending push belongs to another project".into());
        }
        let id = crypto::string(&pending.snapshot, "id")?;
        let (branch, _) = self.read_master_manifest()?;
        if !branch.snapshots.iter().any(|snapshot| snapshot == id) {
            if validate_master(&branch)? != crypto::string(&pending.snapshot, "parent")? {
                let archive = root.join(format!(
                    ".insomnia-sync/{}-rejected.json",
                    workspace::new_id("push")?
                ));
                fs::rename(root.join(PENDING), archive).map_err(|error| error.to_string())?;
                return Err("Master advanced before this push committed. No request changes were committed; retry push to check conflicts".into());
            }
            let key = self.project_key()?;
            let entries: Vec<Entry> = serde_json::from_value(pending.snapshot["state"].clone())
                .map_err(|_| "Invalid pending snapshot".to_string())?;
            let ids: Vec<_> = entries
                .iter()
                .map(|entry| entry.blob.clone())
                .collect::<HashSet<_>>()
                .into_iter()
                .collect();
            for batch in ids.chunks(50) {
                let result = self.query(
                    "query ($projectId: ID!, $ids: [ID!]!) { blobsMissing(project: $projectId, ids: $ids) { missing } }",
                    json!({"projectId": self.project_id, "ids": batch}),
                )?;
                let missing: Vec<String> =
                    serde_json::from_value(result["blobsMissing"]["missing"].clone())
                        .map_err(|_| "Invalid missing-blob response".to_string())?;
                let mut blobs = Vec::new();
                for blob in missing {
                    if !batch.contains(&blob) {
                        return Err("Cloud requested an unexpected blob".into());
                    }
                    let entry = entries
                        .iter()
                        .find(|entry| entry.blob == blob)
                        .ok_or_else(|| "Pending snapshot is missing a blob".to_string())?;
                    let document = pending
                        .documents
                        .get(&entry.key)
                        .ok_or_else(|| "Pending push is missing a resource".to_string())?;
                    let (computed, compressed) = crypto::encode_document(document)?;
                    if computed != blob {
                        return Err("Pending resource does not match its blob ID".into());
                    }
                    blobs.push(json!({"id": blob, "content": crypto::encrypt(&key, &compressed)?.to_string()}));
                }
                // Keep each upload small even when a request has a large body.
                for blob in blobs {
                    self.query("mutation ($projectId: ID!, $blobs: [BlobInput!]!) { blobsCreate(project: $projectId, blobs: $blobs) { count } }",
                        json!({"projectId": self.project_id, "blobs": [blob]}))?;
                }
            }
            let (current, _) = self.read_master_manifest()?;
            if validate_master(&current)? != crypto::string(&pending.snapshot, "parent")? {
                return Err(
                    "Master advanced while uploading; retry push to check conflicts".into(),
                );
            }
            self.query("mutation ($projectId: ID!, $snapshots: [SnapshotInput!]!, $branchName: String!) { snapshotsCreate(project: $projectId, snapshots: $snapshots, branch: $branchName) { id } }",
                json!({"projectId": self.project_id, "branchName": MASTER, "snapshots": [pending.snapshot]}))?;
            let (confirmed, _) = self.read_master_manifest()?;
            if !confirmed.snapshots.iter().any(|snapshot| snapshot == id) {
                return Err(
                    "Push could not be confirmed; retry to reconcile the pending push".into(),
                );
            }
        }
        pending.baseline.save(root)?;
        fs::remove_file(root.join(PENDING)).map_err(|error| {
            format!("Push committed, but cannot clear its local recovery file: {error}")
        })?;
        Ok(format!("Pushed collection changes to master; cloud commit {id}. Run fetch to receive any other master changes."))
    }

    pub fn push(&self, root: &Path, dry_run: bool) -> Result<String, String> {
        if root.join(PENDING).exists() {
            if dry_run {
                return Err(
                    "A previous push needs reconciliation; run push before planning another".into(),
                );
            }
            let pending = serde_json::from_slice(
                &fs::read(root.join(PENDING)).map_err(|error| error.to_string())?,
            )
            .map_err(|_| "Invalid pending push; preserve the recovery file".to_string())?;
            return self.publish_pending(root, pending);
        }
        let baseline = Baseline::load(root)?;
        if baseline.project_id != self.project_id {
            return Err("Workspace belongs to another cloud project".into());
        }
        let local = if dry_run {
            let mut local = workspace::local_requests(root)?;
            for request in local.values_mut() {
                if request.insomnia_id.is_none() {
                    request.insomnia_id = Some(workspace::new_id("req")?);
                }
            }
            local
        } else {
            workspace::assign_local_ids(root)?
        };
        let (mut snapshot, mut documents) = self.download_master()?;
        let mut plan = workspace::plan_push(&baseline, &documents, &local)?;
        if plan.changes.is_empty() {
            if !dry_run {
                plan.baseline.head = snapshot.id;
                plan.baseline.save(root)?;
            }
            return Ok("No local request changes to push".into());
        }
        if dry_run {
            let summaries: Vec<_> = plan
                .changes
                .iter()
                .map(|(id, document)| match document {
                    Some(document) => format!(
                        "{}: {} ({id})",
                        if documents.contains_key(id) {
                            "Update"
                        } else {
                            "Create"
                        },
                        document["name"].as_str().unwrap_or("resource")
                    ),
                    None => format!("Delete: {id}"),
                })
                .collect();
            return Ok(format!(
                "Master push plan ({} resources):\n{}",
                summaries.len(),
                summaries.join("\n")
            ));
        }
        for (id, change) in &plan.changes {
            snapshot.state.retain(|entry| entry.key != *id);
            if let Some(document) = change {
                let (blob, _) = crypto::encode_document(document)?;
                snapshot.state.push(Entry {
                    key: id.clone(),
                    blob,
                    name: crypto::string(document, "name")?.into(),
                });
                documents.insert(id.clone(), document.clone());
            } else {
                documents.remove(id);
            }
        }
        let parent = snapshot.id;
        let mut hash = openssl::sha::Sha1::new();
        hash.update(self.project_id.as_bytes());
        hash.update(parent.as_bytes());
        let mut blobs: Vec<_> = snapshot
            .state
            .iter()
            .map(|entry| entry.blob.as_str())
            .collect();
        blobs.sort_unstable();
        for blob in blobs {
            hash.update(blob.as_bytes());
        }
        let id = crypto::hex(&hash.finish());
        plan.baseline.head = id.clone();
        let created = time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .map_err(|error| error.to_string())?;
        let pending = PendingPush {
            project_id: self.project_id.clone(),
            snapshot: json!({"id": id, "parent": parent, "created": created, "author": crypto::string(&self.session, "accountId")?,
                "name": "Update requests from Mercury", "description": "", "state": snapshot.state}),
            documents,
            baseline: plan.baseline,
        };
        if workspace::local_requests(root)? != local {
            return Err(
                "Local requests changed while planning; retry push after saving edits".into(),
            );
        }
        workspace::atomic_write(
            &root.join(PENDING),
            &serde_json::to_vec_pretty(&pending).map_err(|error| error.to_string())?,
        )?;
        self.publish_pending(root, pending)
    }
}

fn validate_master(branch: &Branch) -> Result<&str, String> {
    if branch.name != MASTER {
        return Err("Cloud returned a branch other than master".into());
    }
    branch
        .snapshots
        .last()
        .map(String::as_str)
        .filter(|head| !head.is_empty())
        .ok_or_else(|| "Remote master has no commits".into())
}

pub fn run_cli(args: &[String]) -> Option<Result<String, String>> {
    let command = args.first()?;
    if !command.starts_with("--insomnia-") {
        return None;
    }
    Some((|| {
        let required = match command.as_str() {
            "--insomnia-status" => 2,
            "--insomnia-fetch" | "--insomnia-push" | "--insomnia-plan" => 3,
            _ => return Err("Unknown Insomnia command".into()),
        };
        if args.len() != required {
            return Err(format!(
                "Usage: mercury {command} <cloud-project-id>{}",
                if required == 3 {
                    " <workspace-folder>"
                } else {
                    ""
                }
            ));
        }
        let directory = dirs::config_dir()
            .ok_or_else(|| "Cannot locate Insomnia's local data".to_string())?
            .join("Insomnia");
        let cloud = Cloud::from_local_session(&args[1], &directory)?;
        if command == "--insomnia-status" {
            let (branch, snapshot) = cloud.read_master_manifest()?;
            return Ok(format!(
                "{}: {} commits, {} resources, {} requests; head {}",
                branch.name,
                branch.snapshots.len(),
                snapshot.state.len(),
                snapshot
                    .state
                    .iter()
                    .filter(|entry| entry.key.starts_with("req_"))
                    .count(),
                snapshot.id
            ));
        }
        let root = std::env::current_dir()
            .map_err(|error| error.to_string())?
            .join(&args[2]);
        let _lock = workspace::SyncLock::acquire(&root)?;
        if command == "--insomnia-push" || command == "--insomnia-plan" {
            return cloud.push(&root, command == "--insomnia-plan");
        }
        if root.join(PENDING).exists() {
            return Err("A pending push must be reconciled before fetching; run push first".into());
        }
        let (snapshot, documents) = cloud.download_master()?;
        let (baseline, warnings) =
            crate::insomnia_workspace::Baseline::from_master(&args[1], &snapshot.id, documents)?;
        let backup = crate::insomnia_workspace::install_master(&root, &baseline, &warnings)?;
        Ok(format!("Fetched {} requests from master into {}. {} execution warnings in .insomnia-sync/warnings.txt.{}",
            baseline.files.len(), root.display(), warnings.len(), if backup.exists() { format!(" Previous workspace: {}", backup.display()) } else { String::new() }))
    })())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_log_uses_latest_record_and_honors_deletion() {
        let content =
            "{\"_id\":\"session\",\"id\":\"old\"}\n{\"_id\":\"session\",\"id\":\"current\"}\n";
        assert_eq!(read_session(content).unwrap()["id"], "current");
        assert!(read_session(&format!(
            "{content}{{\"_id\":\"session\",\"$$deleted\":true}}\n"
        ))
        .is_err());
        assert!(read_session("{}").is_err());
        assert!(read_session("invalid").is_err());
    }

    #[test]
    fn fetch_rejects_other_branches_and_empty_history() {
        let mut branch = Branch {
            name: MASTER.into(),
            snapshots: vec!["head".into()],
        };
        assert_eq!(validate_master(&branch).unwrap(), "head");
        branch.name = "refactor_env".into();
        assert!(validate_master(&branch).is_err());
        branch.name = MASTER.into();
        branch.snapshots.clear();
        assert!(validate_master(&branch).is_err());
    }

    fn recovery_cloud(head: &str) -> Cloud {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let head = head.to_string();
        std::thread::spawn(move || {
            for _ in 0..2 {
                let (mut socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut request = Vec::new();
                let mut buffer = [0; 4096];
                loop {
                    let count = socket.read(&mut buffer).unwrap();
                    assert!(count > 0);
                    request.extend_from_slice(&buffer[..count]);
                    if let Some(header_end) =
                        request.windows(4).position(|bytes| bytes == b"\r\n\r\n")
                    {
                        let headers = String::from_utf8_lossy(&request[..header_end]);
                        let length: usize = headers
                            .lines()
                            .find_map(|line| {
                                line.to_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(str::trim)
                                    .map(str::parse::<usize>)
                                    .map(Result::unwrap)
                            })
                            .unwrap();
                        if request.len() >= header_end + 4 + length {
                            break;
                        }
                    }
                }
                let header_end = request
                    .windows(4)
                    .position(|bytes| bytes == b"\r\n\r\n")
                    .unwrap();
                let body: Value = serde_json::from_slice(&request[header_end + 4..]).unwrap();
                let query = body["query"].as_str().unwrap();
                assert!(
                    !query.contains("mutation"),
                    "reconciliation must not create another commit"
                );
                let data = if query.contains("snapshots(project") {
                    json!({"snapshots": [{"id": head, "state": []}]})
                } else {
                    assert_eq!(body["variables"]["branch"], MASTER);
                    json!({"branch": {"name": MASTER, "snapshots": ["original", head]}})
                };
                let body = json!({"data": data}).to_string();
                write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}", body.len(), body).unwrap();
            }
        });
        Cloud {
            client: Client::new(),
            session: json!({"id": "example"}),
            project_id: "prj_example".into(),
            endpoint: Some(endpoint),
        }
    }

    fn recovery_pending() -> PendingPush {
        PendingPush {
            project_id: "prj_example".into(),
            snapshot: json!({"id": "accepted", "parent": "original", "state": []}),
            documents: BTreeMap::new(),
            baseline: Baseline {
                project_id: "prj_example".into(),
                head: "accepted".into(),
                root_id: "wrk_example".into(),
                documents: BTreeMap::new(),
                files: BTreeMap::new(),
                groups: BTreeMap::new(),
            },
        }
    }

    #[test]
    fn retry_after_accepted_commit_saves_baseline_without_republishing() {
        let root = tempfile::tempdir().unwrap();
        let pending = recovery_pending();
        workspace::atomic_write(
            &root.path().join(PENDING),
            &serde_json::to_vec(&pending).unwrap(),
        )
        .unwrap();
        recovery_cloud("accepted")
            .publish_pending(root.path(), pending)
            .unwrap();
        assert_eq!(Baseline::load(root.path()).unwrap().head, "accepted");
        assert!(!root.path().join(PENDING).exists());
    }

    #[test]
    fn retry_with_diverging_master_archives_uncommitted_push() {
        let root = tempfile::tempdir().unwrap();
        let pending = recovery_pending();
        workspace::atomic_write(
            &root.path().join(PENDING),
            &serde_json::to_vec(&pending).unwrap(),
        )
        .unwrap();
        assert!(recovery_cloud("teammate")
            .publish_pending(root.path(), pending)
            .is_err());
        assert!(!root.path().join(workspace::STATE).exists());
        assert!(!root.path().join(PENDING).exists());
        assert_eq!(
            fs::read_dir(root.path().join(".insomnia-sync"))
                .unwrap()
                .count(),
            1
        );
    }
}
