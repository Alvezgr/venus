//! Projection and baseline for the local Cloud Sync workspace.

use crate::{
    insomnia_crypto as crypto, kv,
    model::{HttpMethod, RequestFile},
    vars,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashSet},
    fs,
    io::Write,
    path::{Path, PathBuf},
};

pub const STATE: &str = ".insomnia-sync/baseline.json";

pub fn push_fingerprint(root: &Path) -> Result<String, String> {
    let baseline = Baseline::load(root)?;
    let mut requests = local_requests(root)?;
    for request in requests.values_mut() {
        if request
            .insomnia_id
            .as_ref()
            .is_none_or(|id| !baseline.files.contains_key(id))
        {
            request.insomnia_id = None;
        }
    }
    let environments = baseline.local_environments(root)?;
    let bytes = serde_json::to_vec(&(baseline, requests, environments))
        .map_err(|error| error.to_string())?;
    Ok(crypto::hex(&openssl::sha::sha256(&bytes)))
}

pub struct SyncLock(fs::File);

impl SyncLock {
    pub fn acquire(root: &Path) -> Result<Self, String> {
        if fs::symlink_metadata(root).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
            return Err("The sync workspace cannot be a symbolic link".into());
        }
        let parent = root
            .parent()
            .ok_or_else(|| "Invalid workspace folder".to_string())?;
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        let name = root
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| "Invalid workspace name".to_string())?;
        // Keep the lock name shared with older Mercury builds using the same workspace.
        let path = parent.join(format!(".{name}.mercury-sync.lock"));
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|error| format!("Cannot open workspace lock: {error}: {}", path.display()))?;
        file.try_lock().map_err(|error| {
            format!("Another sync is running, or the workspace cannot be locked: {error}")
        })?;
        Ok(Self(file))
    }
}

impl Drop for SyncLock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Tracked {
    pub path: String,
    pub request: RequestFile,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Baseline {
    pub project_id: String,
    pub head: String,
    pub root_id: String,
    pub documents: BTreeMap<String, Value>,
    pub files: BTreeMap<String, Tracked>,
    pub groups: BTreeMap<String, String>,
    #[serde(default)]
    pub environments: BTreeMap<String, String>,
}

pub fn new_id(prefix: &str) -> Result<String, String> {
    let mut bytes = [0; 16];
    openssl::rand::rand_bytes(&mut bytes)
        .map_err(|_| "Cannot generate cloud resource identity".to_string())?;
    Ok(format!("{prefix}_{}", crypto::hex(&bytes)))
}

pub fn atomic_write(path: &Path, content: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "Invalid workspace path".to_string())?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let temporary = parent.join(new_id(".venus-write")?);
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temporary)
        .map_err(|error| error.to_string())?;
    let result = file
        .write_all(content)
        .and_then(|()| file.sync_all())
        .and_then(|()| fs::rename(&temporary, path));
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(|error| format!("Cannot save {}: {error}", path.display()))
}

impl Baseline {
    pub fn load(root: &Path) -> Result<Self, String> {
        let bytes = fs::read(root.join(STATE))
            .map_err(|_| "Fetch master into this workspace first".to_string())?;
        let baseline: Self = serde_json::from_slice(&bytes)
            .map_err(|_| "Invalid sync baseline; restore the workspace backup".to_string())?;
        for tracked in baseline.files.values() {
            safe_relative(Path::new(&tracked.path))?;
        }
        let mut environment_paths = HashSet::new();
        for (id, path) in &baseline.environments {
            safe_relative(Path::new(path))?;
            if !path.starts_with(".env.insomnia-")
                || Path::new(path).components().count() != 1
                || !environment_paths.insert(path)
                || baseline.documents.get(id).is_none_or(|document| {
                    document["type"] != "Environment" || !document["data"].is_object()
                })
            {
                return Err("Invalid cloud environment path".into());
            }
        }
        for group in baseline.groups.keys() {
            safe_relative(Path::new(group))?;
        }
        Ok(baseline)
    }

    pub fn save(&self, root: &Path) -> Result<(), String> {
        atomic_write(
            &root.join(STATE),
            &serde_json::to_vec_pretty(self).map_err(|error| error.to_string())?,
        )
    }

    pub fn from_master(
        project_id: &str,
        head: &str,
        documents: BTreeMap<String, Value>,
    ) -> Result<(Self, Vec<String>), String> {
        let roots: Vec<_> = documents
            .values()
            .filter(|document| document["type"] == "Workspace")
            .collect();
        if roots.len() != 1 {
            return Err("Expected one Insomnia collection in this cloud project".into());
        }
        let root_id = crypto::string(roots[0], "_id")?.to_string();
        let mut baseline = Self {
            project_id: project_id.into(),
            head: head.into(),
            root_id,
            documents,
            files: BTreeMap::new(),
            groups: BTreeMap::new(),
            environments: BTreeMap::new(),
        };
        let mut paths = BTreeMap::new();
        let mut warnings = Vec::new();
        let mut used = HashSet::new();
        let ids: Vec<_> = baseline.documents.keys().cloned().collect();
        for id in &ids {
            if baseline.documents[id]["type"] == "RequestGroup" {
                let path = resource_path(
                    id,
                    &baseline.documents,
                    &baseline.root_id,
                    &mut paths,
                    &mut HashSet::new(),
                    &mut used,
                )?;
                baseline.groups.insert(path, id.clone());
            }
        }
        for id in ids {
            if baseline.documents[&id]["type"] != "Request" {
                continue;
            }
            let mut path = resource_path(
                &id,
                &baseline.documents,
                &baseline.root_id,
                &mut paths,
                &mut HashSet::new(),
                &mut used,
            )?;
            path.push_str(".json");
            let request =
                project_request(&baseline.documents[&id], &baseline.documents, &mut warnings)?;
            baseline.files.insert(id, Tracked { path, request });
        }
        for (id, document) in &baseline.documents {
            if document["type"] == "Environment" {
                if !document["data"].is_object() {
                    return Err(format!("Environment {id} data must be a JSON object"));
                }
                let path = format!(".env.insomnia-{}-{}.json", name(document), id);
                safe_relative(Path::new(&path))?;
                if Path::new(&path).components().count() != 1 {
                    return Err("Invalid environment identity".into());
                }
                baseline.environments.insert(id.clone(), path);
            }
        }
        Ok((baseline, warnings))
    }

    pub fn is_clean(&self, root: &Path) -> Result<bool, String> {
        let environments = self.local_environments(root)?;
        if self
            .environments
            .keys()
            .any(|id| environments.get(id) != Some(&self.documents[id]["data"]))
        {
            return Ok(false);
        }
        let local = local_requests(root)?;
        if local.len() != self.files.len() {
            return Ok(false);
        }
        let by_id: BTreeMap<_, _> = local
            .iter()
            .filter_map(|(path, request)| {
                request.insomnia_id.as_ref().map(|id| (id, (path, request)))
            })
            .collect();
        Ok(self.files.iter().all(|(id, tracked)| {
            by_id.get(id).is_some_and(|(path, request)| {
                *path == &tracked.path && *request == &tracked.request
            })
        }))
    }
}

fn safe_relative(path: &Path) -> Result<(), String> {
    if path
        .components()
        .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Err("Sync paths must stay inside the workspace".into());
    }
    Ok(())
}

fn name(document: &Value) -> String {
    let raw = document["name"].as_str().unwrap_or("Untitled");
    let cleaned: String = raw
        .chars()
        .map(|character| {
            if character.is_control() || "/\\:*?\"<>|".contains(character) {
                '-'
            } else {
                character
            }
        })
        .collect();
    let cleaned = cleaned.trim().trim_matches('.');
    if cleaned.is_empty() {
        "Untitled".into()
    } else {
        cleaned.chars().take(100).collect()
    }
}

fn resource_path(
    id: &str,
    documents: &BTreeMap<String, Value>,
    root_id: &str,
    paths: &mut BTreeMap<String, String>,
    visiting: &mut HashSet<String>,
    used: &mut HashSet<String>,
) -> Result<String, String> {
    if id == root_id {
        return Ok(String::new());
    }
    if let Some(path) = paths.get(id) {
        return Ok(path.clone());
    }
    if !visiting.insert(id.into()) || visiting.len() > 64 {
        return Err("Cloud collection has cyclic or excessively deep folders".into());
    }
    let document = documents
        .get(id)
        .ok_or_else(|| "Cloud collection has an orphan resource".to_string())?;
    let parent = crypto::string(document, "parentId")?;
    if parent != root_id
        && documents
            .get(parent)
            .is_none_or(|parent| parent["type"] != "RequestGroup")
    {
        return Err("Cloud request parent is not a collection folder".into());
    }
    let directory = resource_path(parent, documents, root_id, paths, visiting, used)?;
    let stem = name(document);
    let base = if directory.is_empty() {
        stem
    } else {
        format!("{directory}/{stem}")
    };
    let mut path = base.clone();
    let mut suffix = 2;
    while !used.insert(path.to_lowercase()) {
        path = format!("{base} ({suffix})");
        suffix += 1;
    }
    visiting.remove(id);
    paths.insert(id.into(), path.clone());
    Ok(path)
}

pub fn venus_templates(text: &str) -> String {
    let mappings = vars::extract(text)
        .into_iter()
        .filter_map(|variable| {
            variable
                .strip_prefix("_.")
                .map(|key| (variable.clone(), format!("{{{{{key}}}}}")))
        })
        .collect();
    vars::substitute(text, &mappings)
}

pub fn cloud_templates(text: &str) -> String {
    let mappings = vars::extract(text)
        .into_iter()
        .filter(|variable| {
            !variable.starts_with("_.")
                && variable
                    .chars()
                    .all(|character| character.is_alphanumeric() || "_.-".contains(character))
        })
        .map(|variable| (variable.clone(), format!("{{{{ _.{variable} }}}}")))
        .collect();
    vars::substitute(text, &mappings)
}

fn auth<'a>(document: &'a Value, documents: &'a BTreeMap<String, Value>) -> &'a Value {
    let mut current = document;
    let mut seen = HashSet::new();
    loop {
        let authentication = &current["authentication"];
        if authentication["disabled"] == true {
            return authentication;
        }
        if authentication["type"]
            .as_str()
            .is_some_and(|kind| !kind.is_empty())
        {
            return authentication;
        }
        let Some(parent) = current["parentId"].as_str() else {
            return authentication;
        };
        if !seen.insert(parent) {
            return authentication;
        }
        let Some(parent) = documents.get(parent) else {
            return authentication;
        };
        current = parent;
    }
}

pub fn project_request(
    document: &Value,
    documents: &BTreeMap<String, Value>,
    warnings: &mut Vec<String>,
) -> Result<RequestFile, String> {
    let id = crypto::string(document, "_id")?;
    let mut headers: BTreeMap<String, String> = BTreeMap::new();
    for header in document["headers"].as_array().into_iter().flatten() {
        if header["disabled"] == true {
            continue;
        }
        let key = crypto::string(header, "name")?;
        if headers
            .insert(
                key.into(),
                venus_templates(crypto::string(header, "value")?),
            )
            .is_some()
        {
            warnings.push(format!(
                "{id}: duplicate headers cannot be represented in Venus"
            ));
        }
    }
    let authentication = auth(document, documents);
    if authentication["disabled"] != true {
        let value = match authentication["type"].as_str().unwrap_or("") {
            "bearer" => Some(format!(
                "{}{}{}",
                authentication["prefix"]
                    .as_str()
                    .filter(|prefix| !prefix.is_empty())
                    .unwrap_or("Bearer"),
                " ",
                venus_templates(authentication["token"].as_str().unwrap_or(""))
            )),
            "basic" => {
                let username = venus_templates(authentication["username"].as_str().unwrap_or(""));
                let password = venus_templates(authentication["password"].as_str().unwrap_or(""));
                if username.contains("{{") || password.contains("{{") {
                    warnings.push(format!("{id}: Basic auth contains templates; configure the Authorization header locally"));
                    None
                } else {
                    Some(kv::basic_auth(&username, &password))
                }
            }
            "" => None,
            kind => {
                warnings.push(format!(
                    "{id}: {kind} auth is preserved but not executed by Venus"
                ));
                None
            }
        };
        if let Some(value) = value {
            headers.retain(|key, _| !key.eq_ignore_ascii_case("Authorization"));
            headers.insert("Authorization".into(), value);
        }
    }
    let mut url = venus_templates(crypto::string(document, "url")?);
    for parameter in document["parameters"].as_array().into_iter().flatten() {
        if parameter["disabled"] == true {
            continue;
        }
        let mut parameters = kv::parse_query_params(&url);
        parameters.push(kv::KeyValue::new(
            venus_templates(crypto::string(parameter, "name")?),
            venus_templates(crypto::string(parameter, "value")?),
        ));
        url = kv::build_url(&url, &parameters);
    }
    let body = &document["body"];
    let mut body_text = body["text"]
        .as_str()
        .map(venus_templates)
        .unwrap_or_default();
    let mime = body["mimeType"].as_str().unwrap_or("");
    if mime == "application/x-www-form-urlencoded" {
        let parameters: Vec<_> = body["params"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|parameter| parameter["disabled"] != true)
            .map(|parameter| {
                kv::KeyValue::new(
                    venus_templates(parameter["name"].as_str().unwrap_or("")),
                    venus_templates(parameter["value"].as_str().unwrap_or("")),
                )
            })
            .collect();
        body_text = kv::build_form(&parameters);
    } else if body.get("params").is_some() || body.get("fileName").is_some() {
        warnings.push(format!(
            "{id}: multipart/file body is preserved but not executed by Venus"
        ));
    }
    if !mime.is_empty()
        && !headers
            .keys()
            .any(|key| key.eq_ignore_ascii_case("Content-Type"))
    {
        headers.insert("Content-Type".into(), mime.into());
    }
    if ["preRequestScript", "afterResponseScript"]
        .iter()
        .any(|key| {
            document[*key]
                .as_str()
                .is_some_and(|script| !script.trim().is_empty())
        })
        || document["scripts"].as_object().is_some_and(|scripts| {
            scripts.values().any(|script| {
                script
                    .as_str()
                    .is_some_and(|script| !script.trim().is_empty())
            })
        })
    {
        warnings.push(format!(
            "{id}: Insomnia scripts are preserved but not executed by Venus"
        ));
    }
    if document.to_string().contains("{%") {
        warnings.push(format!(
            "{id}: Insomnia template tags are preserved but not executed by Venus"
        ));
    }
    if document["pathParameters"]
        .as_array()
        .is_some_and(|parameters| !parameters.is_empty())
    {
        warnings.push(format!(
            "{id}: Insomnia path parameters are preserved but not executed by Venus"
        ));
    }
    Ok(RequestFile {
        method: HttpMethod::parse(crypto::string(document, "method")?)
            .ok_or_else(|| format!("{id}: unsupported HTTP method"))?,
        url,
        headers,
        body: body_text,
        insomnia_id: Some(id.into()),
    })
}

pub fn local_requests(root: &Path) -> Result<BTreeMap<String, RequestFile>, String> {
    fn walk(
        root: &Path,
        directory: &Path,
        depth: usize,
        output: &mut BTreeMap<String, RequestFile>,
        ids: &mut HashSet<String>,
    ) -> Result<(), String> {
        if depth > 64 {
            return Err("Workspace folders are too deep".into());
        }
        for entry in fs::read_dir(directory).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            if entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            let kind = entry.file_type().map_err(|error| error.to_string())?;
            if kind.is_symlink() {
                return Err("Sync workspaces cannot contain symbolic links".into());
            }
            let path = entry.path();
            if kind.is_dir() {
                walk(root, &path, depth + 1, output, ids)?;
            } else if path
                .extension()
                .is_some_and(|extension| extension == "json")
            {
                let request = RequestFile::from_json(
                    &fs::read_to_string(&path).map_err(|error| error.to_string())?,
                )?;
                if let Some(id) = &request.insomnia_id {
                    if !ids.insert(id.clone()) {
                        return Err(format!("Duplicate Insomnia request identity: {id}"));
                    }
                }
                let relative = path.strip_prefix(root).map_err(|error| error.to_string())?;
                let relative = relative
                    .to_str()
                    .ok_or_else(|| "Request paths must be UTF-8".to_string())?
                    .replace('\\', "/");
                output.insert(relative, request);
            }
        }
        Ok(())
    }
    let mut output = BTreeMap::new();
    if root.exists() {
        walk(root, root, 0, &mut output, &mut HashSet::new())?;
    }
    Ok(output)
}

pub fn assign_local_ids(root: &Path) -> Result<BTreeMap<String, RequestFile>, String> {
    let mut local = local_requests(root)?;
    for (path, request) in &mut local {
        if request.insomnia_id.is_none() {
            request.insomnia_id = Some(new_id("req")?);
            atomic_write(&root.join(path), request.to_json().as_bytes())?;
        }
    }
    Ok(local)
}

pub fn edit_document(
    document: &Value,
    before: &RequestFile,
    after: &RequestFile,
) -> Result<Value, String> {
    let mut edited = document.clone();
    if before.method != after.method {
        edited["method"] = json!(after.method.as_str());
    }
    if before.url != after.url {
        edited["url"] = json!(cloud_templates(&after.url));
        // Enabled query parameters are already projected into Venus's URL.
        edited["parameters"] = json!(document["parameters"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|parameter| parameter["disabled"] == true)
            .cloned()
            .collect::<Vec<_>>());
    }
    if before.headers != after.headers {
        let mut names = HashSet::new();
        for header in document["headers"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|header| header["disabled"] != true)
        {
            if !names.insert(crypto::string(header, "name")?.to_lowercase()) {
                return Err("Edit duplicate request headers in Insomnia; Venus's header map cannot preserve them".into());
            }
        }
        let before_auth = kv::auth_value(&kv::map_to_headers(&before.headers)).map(str::to_string);
        let after_auth = kv::auth_value(&kv::map_to_headers(&after.headers)).map(str::to_string);
        let auth_changed = before_auth != after_auth;
        if auth_changed {
            if !edited["authentication"].is_object() {
                edited["authentication"] = json!({});
            }
            edited["authentication"]["disabled"] = json!(true);
        }
        let mut headers: Vec<Value> = document["headers"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|header| header["disabled"] == true)
            .cloned()
            .collect();
        for (name, value) in &after.headers {
            let original = document["headers"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|header| {
                    header["disabled"] != true
                        && header["name"]
                            .as_str()
                            .is_some_and(|original| original.eq_ignore_ascii_case(name))
                });
            // Keep synthesized auth/content-type in their original Insomnia fields.
            if !auth_changed && name.eq_ignore_ascii_case("Authorization") && original.is_none() {
                continue;
            }
            if name.eq_ignore_ascii_case("Content-Type")
                && original.is_none()
                && before.headers.get(name) == Some(value)
            {
                continue;
            }
            let mut header = original.cloned().unwrap_or_else(|| json!({}));
            header["name"] = json!(name);
            if before.headers.get(name) != Some(value) || original.is_none() {
                header["value"] = json!(cloud_templates(value));
            }
            headers.push(header);
        }
        edited["headers"] = json!(headers);
    }
    if before.body != after.body {
        let mime = document["body"]["mimeType"].as_str().unwrap_or("");
        if document["body"].get("fileName").is_some() || mime == "multipart/form-data" {
            return Err(
                "Venus cannot edit multipart/file bodies; edit that body in Insomnia".into(),
            );
        }
        if !edited["body"].is_object() {
            edited["body"] = json!({});
        }
        if mime == "application/x-www-form-urlencoded" {
            let mut parameters: Vec<Value> = document["body"]["params"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|parameter| parameter["disabled"] == true)
                .cloned()
                .collect();
            for row in kv::parse_form(&after.body) {
                let mut parameter = document["body"]["params"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|parameter| parameter["name"] == row.key && parameter["disabled"] != true)
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                parameter["name"] = json!(row.key);
                parameter["value"] = json!(cloud_templates(&row.value));
                parameters.push(parameter);
            }
            edited["body"]["params"] = json!(parameters);
        } else {
            edited["body"]["text"] = json!(cloud_templates(&after.body));
            if mime.is_empty() {
                let header_text = kv::map_to_headers(&after.headers);
                edited["body"]["mimeType"] =
                    json!(kv::header_value(&header_text, "Content-Type").unwrap_or("text/plain"));
            }
        }
    }
    Ok(edited)
}

pub struct PushPlan {
    pub changes: BTreeMap<String, Option<Value>>,
    pub baseline: Baseline,
}

impl Baseline {
    pub fn local_environments(&self, root: &Path) -> Result<BTreeMap<String, Value>, String> {
        let mut values = BTreeMap::new();
        for (id, path) in &self.environments {
            let file = root.join(path);
            let metadata = match fs::symlink_metadata(&file) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.to_string()),
            };
            if !metadata.is_file() || metadata.len() > 32 * 1024 * 1024 {
                return Err(format!(
                    "Environment {path} must be a regular file under 32 MB"
                ));
            }
            let data: Value =
                serde_json::from_slice(&fs::read(file).map_err(|error| error.to_string())?)
                    .map_err(|_| format!("Environment {path} must contain valid JSON"))?;
            if !data.is_object() {
                return Err(format!("Environment {path} must contain a JSON object"));
            }
            values.insert(id.clone(), data);
        }
        Ok(values)
    }
}

impl PushPlan {
    pub fn add_environments(
        &mut self,
        baseline: &Baseline,
        remote: &BTreeMap<String, Value>,
        local: &BTreeMap<String, Value>,
    ) -> Result<(), String> {
        for (id, path) in &baseline.environments {
            let original = baseline
                .documents
                .get(id)
                .ok_or_else(|| "Sync baseline is missing an environment".to_string())?;
            if local.get(id) == Some(&original["data"]) {
                continue;
            }
            if remote.get(id).is_some_and(|current| current != original)
                || (local.contains_key(id) && !remote.contains_key(id))
            {
                return Err(format!("Conflict: environment {path} changed on master"));
            }
            if let Some(data) = local.get(id) {
                let mut edited = original.clone();
                edited["data"] = data.clone();
                edited["modified"] = json!((crate::storage::now() * 1000.0) as u64);
                self.changes.insert(id.clone(), Some(edited.clone()));
                self.baseline.documents.insert(id.clone(), edited);
            } else {
                if original["parentId"] == baseline.root_id {
                    return Err(format!(
                        "Cannot delete the collection's base environment: {path}"
                    ));
                }
                if remote.values().any(|document| {
                    document["parentId"] == *id
                        && (!baseline
                            .environments
                            .contains_key(document["_id"].as_str().unwrap_or(""))
                            || local.contains_key(document["_id"].as_str().unwrap_or("")))
                }) {
                    return Err(format!(
                        "Cannot delete {path}: it still has child resources"
                    ));
                }
                if remote.contains_key(id) {
                    self.changes.insert(id.clone(), None);
                }
                self.baseline.environments.remove(id);
                self.baseline.documents.remove(id);
            }
        }
        Ok(())
    }
}

pub fn environment_vars(
    root: &Path,
    path: &str,
) -> Result<std::collections::HashMap<String, String>, String> {
    // Ordinary dotenv files remain local and retain their existing parsing rules.
    if !root.join(STATE).exists() {
        return fs::read_to_string(root.join(path))
            .map(|content| vars::parse_env(&content))
            .map_err(|error| error.to_string());
    }
    let baseline = Baseline::load(root)?;
    let Some((id, _)) = baseline
        .environments
        .iter()
        .find(|(_, tracked)| *tracked == path)
    else {
        return fs::read_to_string(root.join(path))
            .map(|content| vars::parse_env(&content))
            .map_err(|error| error.to_string());
    };
    let local = baseline.local_environments(root)?;
    let mut chain = Vec::new();
    let mut current = id.as_str();
    let mut visited = HashSet::new();
    while baseline.environments.contains_key(current) {
        if !visited.insert(current) {
            return Err("Environment inheritance contains a cycle".into());
        }
        let data = local
            .get(current)
            .ok_or_else(|| "An inherited environment file is missing".to_string())?;
        chain.push(
            data.as_object()
                .ok_or_else(|| "Invalid environment data".to_string())?,
        );
        current = baseline.documents[current]["parentId"]
            .as_str()
            .unwrap_or("");
    }
    let mut values = std::collections::HashMap::new();
    for data in chain.into_iter().rev() {
        for (key, value) in data {
            values.insert(
                key.clone(),
                venus_templates(
                    &value
                        .as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| value.to_string()),
                ),
            );
        }
    }
    Ok(values)
}

pub fn plan_push(
    baseline: &Baseline,
    remote: &BTreeMap<String, Value>,
    local: &BTreeMap<String, RequestFile>,
) -> Result<PushPlan, String> {
    let mut changes = BTreeMap::new();
    if remote
        .get(&baseline.root_id)
        .is_none_or(|document| document["type"] != "Workspace")
    {
        return Err(
            "The cloud collection was removed or replaced; fetch into a new workspace".into(),
        );
    }
    let mut next = baseline.clone();
    let mut local_ids = HashSet::new();
    for (path, request) in local {
        safe_relative(Path::new(path))?;
        let id = request
            .insomnia_id
            .as_deref()
            .ok_or_else(|| "Local request has no sync identity".to_string())?;
        if !id.starts_with("req_")
            || id.len() > 80
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
            || !local_ids.insert(id)
        {
            return Err("Invalid or duplicated Insomnia request identity".into());
        }
        if let Some(tracked) = baseline.files.get(id) {
            if tracked.path == *path && tracked.request == *request {
                continue;
            }
            let original = baseline
                .documents
                .get(id)
                .ok_or_else(|| "Sync baseline is missing a request".to_string())?;
            let mut edited = edit_document(original, &tracked.request, request)?;
            let old_path = Path::new(&tracked.path);
            let new_path = Path::new(path);
            if old_path.file_name() != new_path.file_name() {
                edited["name"] = json!(new_path
                    .file_stem()
                    .and_then(|name| name.to_str())
                    .ok_or_else(|| "Invalid request name".to_string())?);
            }
            if old_path.parent() != new_path.parent() {
                let parent = ensure_group(
                    new_path.parent().unwrap_or(Path::new("")),
                    &mut next,
                    remote,
                    &mut changes,
                )?;
                edited["parentId"] = json!(parent);
            }
            if edited != *original {
                if remote.get(id) != Some(original) {
                    return Err(format!("Conflict: {path} changed on master; preserve your edit and fetch before resolving"));
                }
                changes.insert(id.into(), Some(edited.clone()));
                next.documents.insert(id.into(), edited);
            }
        } else {
            if remote.contains_key(id) {
                return Err(format!(
                    "Request identity {id} belongs to an untracked cloud resource"
                ));
            }
            let parent = ensure_group(
                Path::new(path).parent().unwrap_or(Path::new("")),
                &mut next,
                remote,
                &mut changes,
            )?;
            let created = (crate::storage::now() * 1000.0) as u64;
            let blank = json!({"_id": id, "type": "Request", "created": created,
                "parentId": parent, "name": Path::new(path).file_stem().and_then(|name| name.to_str()).unwrap_or("New Request"),
                "description": "", "method": "GET", "url": "", "headers": [], "parameters": [], "body": {},
                "authentication": {}, "isPrivate": false, "metaSortKey": -(created as i64),
                "settingStoreCookies": true, "settingSendCookies": true, "settingDisableRenderRequestBody": false,
                "settingEncodeUrl": true, "settingRebuildPath": true, "settingFollowRedirects": "global"});
            let edited = edit_document(&blank, &RequestFile::default(), request)?;
            changes.insert(id.into(), Some(edited.clone()));
            next.documents.insert(id.into(), edited);
        }
        next.files.insert(
            id.into(),
            Tracked {
                path: path.clone(),
                request: request.clone(),
            },
        );
    }
    for (id, tracked) in &baseline.files {
        if local_ids.contains(id.as_str()) {
            continue;
        }
        if let Some(current) = remote.get(id) {
            if baseline.documents.get(id) != Some(current) {
                return Err(format!(
                    "Conflict: deleted request {} changed on master",
                    tracked.path
                ));
            }
            changes.insert(id.clone(), None);
        }
        next.documents.remove(id);
        next.files.remove(id);
    }
    Ok(PushPlan {
        changes,
        baseline: next,
    })
}

fn ensure_group(
    path: &Path,
    baseline: &mut Baseline,
    remote: &BTreeMap<String, Value>,
    changes: &mut BTreeMap<String, Option<Value>>,
) -> Result<String, String> {
    // shortcut: sync request locations, not empty folder edits; extend if folder sync is requested.
    if path.as_os_str().is_empty() {
        return Ok(baseline.root_id.clone());
    }
    safe_relative(path)?;
    let name = path
        .to_str()
        .ok_or_else(|| "Folder paths must be UTF-8".to_string())?
        .replace('\\', "/");
    if let Some(id) = baseline.groups.get(&name) {
        if remote
            .get(id)
            .is_some_and(|document| document["type"] == "RequestGroup")
            || changes.get(id).is_some_and(Option::is_some)
        {
            return Ok(id.clone());
        }
        return Err(format!(
            "Folder {name} was removed from master; fetch before moving requests"
        ));
    }
    let parent = ensure_group(
        path.parent().unwrap_or(Path::new("")),
        baseline,
        remote,
        changes,
    )?;
    let id = new_id("fld")?;
    let created = (crate::storage::now() * 1000.0) as u64;
    let document = json!({"_id": id, "type": "RequestGroup", "parentId": parent, "name": path.file_name().and_then(|name| name.to_str()).unwrap_or("Folder"),
        "created": created, "description": "", "environment": {}, "environmentPropertyOrder": null, "authentication": {}, "headers": [], "metaSortKey": -(created as i64)});
    changes.insert(id.clone(), Some(document.clone()));
    baseline.documents.insert(id.clone(), document);
    baseline.groups.insert(name, id.clone());
    Ok(id)
}

pub fn install_master(
    root: &Path,
    baseline: &Baseline,
    warnings: &[String],
) -> Result<PathBuf, String> {
    if root.join(".git").exists() {
        return Err("Use a collection folder, not a Git repository root".into());
    }
    if root.exists() {
        if root.join(STATE).exists() {
            let previous = Baseline::load(root)?;
            if previous.project_id != baseline.project_id || !previous.is_clean(root)? {
                return Err(
                    "Fetch refused: workspace has unpushed changes or belongs to another project"
                        .into(),
                );
            }
        } else if fs::read_dir(root)
            .map_err(|error| error.to_string())?
            .next()
            .is_some()
        {
            return Err("Initial fetch requires an empty workspace folder".into());
        }
    }
    let parent = root
        .parent()
        .ok_or_else(|| "Invalid workspace folder".to_string())?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let stage = parent.join(new_id(".insomnia-fetch")?);
    fs::create_dir(&stage).map_err(|error| error.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&stage, fs::Permissions::from_mode(0o700))
            .map_err(|error| error.to_string())?;
    }
    // Only untracked .env files remain local; replace managed files from master.
    let previous = root
        .join(STATE)
        .exists()
        .then(|| Baseline::load(root))
        .transpose()?;
    if root.exists() {
        for entry in fs::read_dir(root).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            if entry.file_name().to_string_lossy().starts_with(".env")
                && !previous.as_ref().is_some_and(|old| {
                    old.environments
                        .values()
                        .any(|path| Path::new(path) == Path::new(&entry.file_name()))
                })
                && entry
                    .file_type()
                    .map_err(|error| error.to_string())?
                    .is_file()
            {
                atomic_write(
                    &stage.join(entry.file_name()),
                    &fs::read(entry.path()).map_err(|error| error.to_string())?,
                )?;
            }
        }
    }
    for (id, path) in &baseline.environments {
        if stage.join(path).exists() {
            return Err(format!(
                "Cloud environment would overwrite local file {path}"
            ));
        }
        atomic_write(
            &stage.join(path),
            &serde_json::to_vec_pretty(&baseline.documents[id]["data"])
                .map_err(|error| error.to_string())?,
        )?;
    }
    for path in baseline.groups.keys() {
        fs::create_dir_all(stage.join(path)).map_err(|error| error.to_string())?;
    }
    for tracked in baseline.files.values() {
        atomic_write(
            &stage.join(&tracked.path),
            tracked.request.to_json().as_bytes(),
        )?;
    }
    atomic_write(
        &stage.join(".insomnia-sync/warnings.txt"),
        warnings.join("\n").as_bytes(),
    )?;
    baseline.save(&stage)?;
    let backup = parent.join(new_id(".insomnia-backup")?);
    if root.join(STATE).exists() && !Baseline::load(root)?.is_clean(root)? {
        return Err(
            "Local requests or environments changed during fetch; the existing workspace was retained".into(),
        );
    }
    if root.exists() {
        fs::rename(root, &backup).map_err(|error| error.to_string())?;
    }
    if let Err(error) = fs::rename(&stage, root) {
        if backup.exists() {
            let _ = fs::rename(&backup, root);
        }
        return Err(format!("Cannot install fetched workspace: {error}"));
    }
    Ok(backup)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_lock_excludes_parallel_sync_and_releases_on_drop() {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("workspace");
        let lock = SyncLock::acquire(&root).unwrap();
        assert!(SyncLock::acquire(&root).is_err());
        drop(lock);
        assert!(SyncLock::acquire(&root).is_ok());
    }

    fn fixture() -> Baseline {
        let documents = BTreeMap::from([
            (
                "wrk_example".into(),
                json!({"_id": "wrk_example", "type": "Workspace", "name": "Collection", "parentId": null}),
            ),
            (
                "fld_example".into(),
                json!({"_id": "fld_example", "type": "RequestGroup", "name": "API", "parentId": "wrk_example"}),
            ),
            (
                "req_example".into(),
                json!({"_id": "req_example", "type": "Request", "name": "PDF", "parentId": "fld_example",
                "method": "GET", "url": "{{ _.host }}/api/old/pdf/", "authentication": {"type": "bearer", "token": "{{ _.token }}"},
                "headers": [{"name": "X-Disabled", "value": "preserve", "disabled": true}], "parameters": [], "body": {},
                "afterResponseScript": "keepScript()", "settingSendCookies": false}),
            ),
            (
                "env_example".into(),
                json!({"_id": "env_example", "type": "Environment", "data": {"host": "https://example.test"}}),
            ),
        ]);
        Baseline::from_master("prj_example", "head", documents)
            .unwrap()
            .0
    }

    #[test]
    fn preview_fingerprint_covers_changes_but_survives_new_identity_assignment() {
        let baseline = fixture();
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("collection");
        install_master(&root, &baseline, &[]).unwrap();
        let original = push_fingerprint(&root).unwrap();
        atomic_write(
            &root.join("New.json"),
            RequestFile::default().to_json().as_bytes(),
        )
        .unwrap();
        let added = push_fingerprint(&root).unwrap();
        assert_ne!(added, original);
        assign_local_ids(&root).unwrap();
        assert_eq!(push_fingerprint(&root).unwrap(), added);
        atomic_write(
            &root.join(&baseline.environments["env_example"]),
            br#"{"host":"new"}"#,
        )
        .unwrap();
        assert_ne!(push_fingerprint(&root).unwrap(), added);
        let cloud =
            crate::insomnia_sync::Cloud::from_session("prj_example", json!({"id":"example"}))
                .unwrap();
        assert!(cloud
            .push(&root, false, "Update environment", Some(&added))
            .unwrap_err()
            .contains("changed since the preview"));
        assert!(!root.join(crate::insomnia_sync::PENDING).exists());
    }

    #[test]
    fn url_edit_preserves_auth_scripts_disabled_headers_and_environments() {
        let baseline = fixture();
        let tracked = &baseline.files["req_example"];
        let mut request = tracked.request.clone();
        request.url = request.url.replace("/old/", "/new/");
        let local = BTreeMap::from([(tracked.path.clone(), request)]);
        let plan = plan_push(&baseline, &baseline.documents, &local).unwrap();
        assert_eq!(plan.changes.len(), 1);
        let edited = plan.changes["req_example"].as_ref().unwrap();
        assert_eq!(edited["url"], "{{ _.host }}/api/new/pdf/");
        for field in [
            "authentication",
            "afterResponseScript",
            "headers",
            "settingSendCookies",
        ] {
            assert_eq!(edited[field], baseline.documents["req_example"][field]);
        }
        assert_eq!(
            plan.baseline.documents["env_example"],
            baseline.documents["env_example"]
        );
        let mut remote = baseline.documents.clone();
        remote.get_mut("req_example").unwrap()["url"] = json!("remote edit");
        assert!(plan_push(&baseline, &remote, &local).is_err());
    }

    #[test]
    fn unrelated_remote_changes_are_preserved_and_deletion_conflicts_fail() {
        let baseline = fixture();
        let mut remote = baseline.documents.clone();
        remote.get_mut("env_example").unwrap()["data"] = json!({"changed": true});
        let local = BTreeMap::from([(
            baseline.files["req_example"].path.clone(),
            baseline.files["req_example"].request.clone(),
        )]);
        assert!(plan_push(&baseline, &remote, &local)
            .unwrap()
            .changes
            .is_empty());
        let deleted = plan_push(&baseline, &remote, &BTreeMap::new()).unwrap();
        assert_eq!(deleted.changes.len(), 1);
        assert!(deleted.changes["req_example"].is_none());
        remote.get_mut("req_example").unwrap()["name"] = json!("Teammate rename");
        assert!(plan_push(&baseline, &remote, &BTreeMap::new()).is_err());
    }

    #[test]
    fn matching_local_and_remote_deletions_reconciles_without_a_commit() {
        let baseline = fixture();
        let mut remote = baseline.documents.clone();
        remote.remove("req_example");
        let plan = plan_push(&baseline, &remote, &BTreeMap::new()).unwrap();
        assert!(plan.changes.is_empty());
        assert!(!plan.baseline.files.contains_key("req_example"));
        assert!(!plan.baseline.documents.contains_key("req_example"));
        let root = tempfile::tempdir().unwrap();
        for (id, path) in &plan.baseline.environments {
            atomic_write(
                &root.path().join(path),
                &serde_json::to_vec(&plan.baseline.documents[id]["data"]).unwrap(),
            )
            .unwrap();
        }
        plan.baseline.save(root.path()).unwrap();
        assert!(Baseline::load(root.path())
            .unwrap()
            .is_clean(root.path())
            .unwrap());
    }

    #[test]
    fn environments_roundtrip_inherit_and_push_without_losing_types_or_metadata() {
        let mut documents = fixture().documents;
        documents.get_mut("env_example").unwrap()["parentId"] = json!("wrk_example");
        documents.get_mut("env_example").unwrap()["data"] = json!({"host":"https://example.test", "count": 12, "nested": {"a":true}, "secret":"line one\n\"line two\"\\end"});
        documents.insert("env_child".into(), json!({"_id":"env_child", "type":"Environment", "parentId":"env_example", "name":"Development", "color":"#123456", "dataPropertyOrder":{"&":["host"]}, "data":{"host":"http://localhost", "template":"{{ _.host }}"}}));
        let baseline = Baseline::from_master("prj_example", "head", documents)
            .unwrap()
            .0;
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("collection");
        install_master(&root, &baseline, &[]).unwrap();
        assert!(baseline.is_clean(&root).unwrap());
        assert_eq!(
            baseline.local_environments(&root).unwrap()["env_example"],
            baseline.documents["env_example"]["data"]
        );
        let vars = environment_vars(&root, &baseline.environments["env_child"]).unwrap();
        assert_eq!(
            crate::workspace::env_label(&baseline.environments["env_child"]),
            "Development"
        );
        assert_eq!(crate::workspace::env_label(".env.local"), ".env.local");
        assert_eq!(vars["host"], "http://localhost");
        assert_eq!(vars["count"], "12");
        assert_eq!(vars["secret"], "line one\n\"line two\"\\end");
        assert_eq!(vars["template"], "{{host}}");
        let mut local = baseline.local_environments(&root).unwrap();
        local.get_mut("env_child").unwrap()["host"] = json!("https://new.test");
        atomic_write(
            &root.join(&baseline.environments["env_child"]),
            &serde_json::to_vec(&local["env_child"]).unwrap(),
        )
        .unwrap();
        assert!(!baseline.is_clean(&root).unwrap());
        assert!(install_master(&root, &baseline, &[]).is_err());
        let mut plan = plan_push(
            &baseline,
            &baseline.documents,
            &local_requests(&root).unwrap(),
        )
        .unwrap();
        plan.add_environments(&baseline, &baseline.documents, &local)
            .unwrap();
        assert_eq!(plan.changes.len(), 1);
        let edited = plan.changes["env_child"].as_ref().unwrap();
        assert_eq!(edited["color"], "#123456");
        assert_eq!(
            edited["dataPropertyOrder"],
            baseline.documents["env_child"]["dataPropertyOrder"]
        );
        assert_eq!(edited["data"], local["env_child"]);
        plan.baseline.save(&root).unwrap();
        assert!(plan.baseline.is_clean(&root).unwrap());
        let mut remote = baseline.documents.clone();
        remote.get_mut("env_child").unwrap()["data"]["host"] = json!("teammate");
        assert!(plan.add_environments(&baseline, &remote, &local).is_err());
        local.remove("env_example");
        assert!(plan
            .add_environments(&baseline, &baseline.documents, &local)
            .is_err());
    }

    #[test]
    fn environment_deletions_validation_and_old_baselines_are_safe() {
        let baseline = fixture();
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("collection");
        install_master(&root, &baseline, &[]).unwrap();
        let path = root.join(&baseline.environments["env_example"]);
        atomic_write(&path, b"[]").unwrap();
        assert!(baseline.local_environments(&root).is_err());
        atomic_write(&path, b"{broken").unwrap();
        assert!(baseline.local_environments(&root).is_err());
        fs::remove_file(&path).unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("/etc/hostname", &path).unwrap();
            assert!(baseline.local_environments(&root).is_err());
            fs::remove_file(&path).unwrap();
        }
        let mut plan = plan_push(
            &baseline,
            &baseline.documents,
            &local_requests(&root).unwrap(),
        )
        .unwrap();
        plan.add_environments(&baseline, &baseline.documents, &BTreeMap::new())
            .unwrap();
        assert!(plan.changes["env_example"].is_none());
        let mut remote = baseline.documents.clone();
        remote.remove("env_example");
        let mut plan = plan_push(&baseline, &remote, &local_requests(&root).unwrap()).unwrap();
        plan.add_environments(&baseline, &remote, &BTreeMap::new())
            .unwrap();
        assert!(plan.changes.is_empty());
        assert!(plan.baseline.environments.is_empty());
        let mut old = serde_json::to_value(&baseline).unwrap();
        old.as_object_mut().unwrap().remove("environments");
        atomic_write(&root.join(STATE), &serde_json::to_vec(&old).unwrap()).unwrap();
        atomic_write(&root.join(".env.local"), b"host=local-only").unwrap();
        assert!(Baseline::load(&root).unwrap().is_clean(&root).unwrap());
        install_master(&root, &baseline, &[]).unwrap();
        assert_eq!(
            fs::read_to_string(root.join(".env.local")).unwrap(),
            "host=local-only"
        );
        assert!(baseline.is_clean(&root).unwrap());
        let mut cyclic = baseline.clone();
        cyclic.documents.get_mut("env_example").unwrap()["parentId"] = json!("env_example");
        cyclic.save(&root).unwrap();
        assert!(environment_vars(&root, &cyclic.environments["env_example"]).is_err());
        assert!(plan
            .add_environments(&baseline, &cyclic.documents, &BTreeMap::new())
            .is_err());
    }

    #[test]
    fn rename_move_create_and_duplicate_identity_are_handled() {
        let baseline = fixture();
        let original = baseline.files["req_example"].request.clone();
        let mut copy = original.clone();
        copy.insomnia_id = Some("req_new".into());
        let local = BTreeMap::from([
            ("Moved/Renamed.json".into(), original.clone()),
            ("Moved/Copy.json".into(), copy),
        ]);
        let plan = plan_push(&baseline, &baseline.documents, &local).unwrap();
        assert_eq!(plan.changes.len(), 3);
        let renamed = plan.changes["req_example"].as_ref().unwrap();
        assert_eq!(renamed["name"], "Renamed");
        assert_eq!(
            renamed["parentId"],
            plan.changes["req_new"].as_ref().unwrap()["parentId"]
        );
        let duplicate = BTreeMap::from([
            ("one.json".into(), original.clone()),
            ("two.json".into(), original),
        ]);
        assert!(plan_push(&baseline, &baseline.documents, &duplicate).is_err());
    }

    #[test]
    fn fetch_refuses_dirty_workspace_and_retains_backup_and_local_env() {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("collection");
        let baseline = fixture();
        install_master(&root, &baseline, &[]).unwrap();
        assert!(baseline.is_clean(&root).unwrap());
        fs::write(root.join(".env.local"), "LOCAL=value").unwrap();
        let backup = install_master(&root, &baseline, &[]).unwrap();
        assert!(backup.join(STATE).exists());
        assert_eq!(
            fs::read_to_string(root.join(".env.local")).unwrap(),
            "LOCAL=value"
        );
        let tracked = &baseline.files["req_example"];
        let mut edited = tracked.request.clone();
        edited.url = "local edit".into();
        fs::write(root.join(&tracked.path), edited.to_json()).unwrap();
        assert!(install_master(&root, &baseline, &[]).is_err());
        assert_eq!(
            RequestFile::from_json(&fs::read_to_string(root.join(&tracked.path)).unwrap())
                .unwrap()
                .url,
            "local edit"
        );
    }

    #[test]
    fn whitespace_templates_and_unrepresentable_bodies_are_safe() {
        assert_eq!(
            venus_templates("{{  _.host  }}/{{_.id}}"),
            "{{host}}/{{id}}"
        );
        assert_eq!(
            cloud_templates("{{  host  }}/{{id}}"),
            "{{ _.host }}/{{ _.id }}"
        );
        let document = json!({"body": {"mimeType": "multipart/form-data", "params": [{"name": "file", "fileName": "local"}]}});
        let before = RequestFile::default();
        let after = RequestFile {
            body: "replacement".into(),
            ..Default::default()
        };
        assert!(edit_document(&document, &before, &after).is_err());
    }
}
