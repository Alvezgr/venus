//! Sign-in and collection selection for the status-bar sync menu.

use super::*;
use crate::{insomnia_auth as auth, insomnia_sync::Collection};
use serde_json::Value;

pub(super) type AccountResult = Result<(Value, String, Vec<Collection>), String>;

pub(super) struct Picker {
    id: u64,
    pending: bool,
    email: Option<String>,
    collections: Vec<Collection>,
    search: String,
    error: Option<String>,
    challenge: Option<auth::LoginChallenge>,
    code: String,
}

impl MercuryApp {
    pub(super) fn open_cloud_picker(&mut self) {
        self.request_counter += 1;
        let id = self.request_counter;
        self.cloud_picker = Some(Picker {
            id,
            pending: true,
            email: None,
            collections: Vec::new(),
            search: String::new(),
            error: None,
            challenge: None,
            code: String::new(),
        });
        self.spawn(move || Some(Event::CloudAccount(id, load_account(auth::load_session()))));
    }

    pub(super) fn on_cloud_account(&mut self, id: u64, result: AccountResult) {
        let Some(picker) = self.cloud_picker.as_mut().filter(|picker| picker.id == id) else {
            return;
        };
        picker.pending = false;
        match result.and_then(|(session, email, collections)| {
            auth::save_session(&session)?;
            Ok((email, collections))
        }) {
            Ok((email, collections)) => {
                picker.email = Some(email);
                picker.collections = collections;
                picker.error = None;
                picker.challenge = None;
                picker.code.clear();
            }
            Err(error) => picker.error = Some(error),
        }
    }

    pub(super) fn show_cloud_picker(&mut self, ctx: &egui::Context) {
        let Some(picker) = self.cloud_picker.as_mut() else {
            return;
        };
        let mut browser = false;
        let mut native = false;
        let mut sign_out = false;
        let mut complete = false;
        let mut selected = None;
        let action = modal(ctx, "Insomnia Cloud", |ui| {
            if let Some(email) = &picker.email {
                ui.label(muted(format!("Signed in as {email}")));
            } else {
                ui.label(muted(
                    "Sign in to choose a collection. Downloads always use master.",
                ));
            }
            ui.add_space(Space::MD);
            ui.add_enabled_ui(!picker.pending, |ui| {
                ui.horizontal(|ui| {
                    browser = ui.button("Sign in with browser").clicked();
                    native = ui.button("Use Insomnia login").clicked();
                    if picker.email.is_some() {
                        sign_out = ui.button("Sign out").clicked();
                    }
                });
                if picker.challenge.is_some() {
                    ui.add_space(Space::MD);
                    ui.label(
                        "Finish sign-in in your browser, then copy its authentication code here.",
                    );
                    ui.add(
                        egui::TextEdit::singleline(&mut picker.code)
                            .password(true)
                            .hint_text("Authentication code")
                            .desired_width(f32::INFINITY),
                    );
                    complete = ui
                        .add_enabled(
                            !picker.code.trim().is_empty(),
                            egui::Button::new("Complete sign-in"),
                        )
                        .clicked();
                }
            });
            if picker.pending {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Connecting to Insomnia Cloud…");
                });
            }
            if let Some(error) = &picker.error {
                ui.add_space(Space::MD);
                ui.label(egui::RichText::new(error).color(theme().error));
            }
            if picker.email.is_some() && picker.challenge.is_none() {
                ui.add_space(Space::LG);
                ui.add(
                    egui::TextEdit::singleline(&mut picker.search)
                        .hint_text("Find a collection")
                        .desired_width(f32::INFINITY),
                );
                ui.label(muted(
                    "Choose a collection, then an empty local folder to download master.",
                ));
                ui.add_space(Space::SM);
                let filter = picker.search.to_lowercase();
                let matches: Vec<_> = picker
                    .collections
                    .iter()
                    .filter(|collection| {
                        collection.name.to_lowercase().contains(&filter)
                            || collection
                                .teams
                                .iter()
                                .any(|team| team.name.to_lowercase().contains(&filter))
                    })
                    .collect();
                egui::ScrollArea::vertical()
                    .max_height(260.0)
                    .show(ui, |ui| {
                        ui.add_enabled_ui(!picker.pending, |ui| {
                            if matches.is_empty() {
                                ui.label(muted(if picker.collections.is_empty() {
                                    "No cloud collections available for this account."
                                } else {
                                    "No matching collections."
                                }));
                            }
                            for collection in matches {
                                let team = collection
                                    .teams
                                    .first()
                                    .map(|team| team.name.as_str())
                                    .unwrap_or("Personal");
                                if ui
                                    .add_sized(
                                        [ui.available_width(), 38.0],
                                        egui::Button::new(format!(
                                            "{} · {}",
                                            collection.name, team
                                        )),
                                    )
                                    .clicked()
                                {
                                    selected = Some(collection.id.clone());
                                }
                            }
                        });
                    });
            }
            ui.add_space(Space::MD);
            if ui.button("Close").clicked() {
                ModalAction::Close
            } else {
                ModalAction::Open
            }
        });
        if action == ModalAction::Close {
            self.cloud_picker = None;
            return;
        }
        if browser {
            let result = auth::LoginChallenge::new().and_then(|challenge| {
                open::that(challenge.url()).map_err(|_| "Cannot open your browser".to_string())?;
                Ok(challenge)
            });
            let picker = self.cloud_picker.as_mut().unwrap();
            match result {
                Ok(challenge) => {
                    picker.challenge = Some(challenge);
                    picker.code.clear();
                    picker.error = None;
                }
                Err(error) => picker.error = Some(error),
            }
        }
        if native || complete {
            self.request_counter += 1;
            let id = self.request_counter;
            let picker = self.cloud_picker.as_mut().unwrap();
            picker.id = id;
            picker.pending = true;
            picker.error = None;
            let challenge = if complete {
                picker.challenge.clone()
            } else {
                None
            };
            let code = std::mem::take(&mut picker.code);
            self.spawn(move || {
                let session = if native {
                    auth::native_session()
                } else {
                    challenge
                        .ok_or_else(|| "Start browser sign-in again".to_string())
                        .and_then(|challenge| challenge.complete(&code))
                };
                Some(Event::CloudAccount(id, load_account(session)))
            });
        }
        if sign_out {
            match auth::sign_out() {
                Ok(()) => {
                    self.cloud_picker = Some(Picker {
                        id: 0,
                        pending: false,
                        email: None,
                        collections: Vec::new(),
                        search: String::new(),
                        error: None,
                        challenge: None,
                        code: String::new(),
                    })
                }
                Err(error) => self.cloud_picker.as_mut().unwrap().error = Some(error),
            }
        }
        if let Some(project) = selected {
            if self.has_unsaved_changes() && !self.save_file() {
                return;
            }
            self.cloud_picker = None;
            self.sync_busy = Some("Downloading master…");
            self.watcher = None;
            self.spawn(move || {
                let result = rfd::FileDialog::new()
                    .set_title("Choose an empty folder for the collection")
                    .pick_folder()
                    .map(|root| {
                        let args = vec![
                            "--insomnia-fetch".into(),
                            project,
                            root.to_string_lossy().into_owned(),
                        ];
                        crate::insomnia_sync::run_cli(&args)
                            .unwrap()
                            .map(|message| (root, message))
                    })
                    .transpose();
                Some(Event::CloudDownloaded(result))
            });
        }
    }

    pub(super) fn on_cloud_download(&mut self, result: Result<Option<(PathBuf, String)>, String>) {
        self.sync_busy = None;
        match result {
            Ok(Some((root, message))) => {
                self.load_request(Request::default(), None);
                self.open_workspace(root.clone(), None);
                if let Some(CollectionItem::Request { path, .. }) =
                    workspace::all_requests(&root).into_iter().next()
                {
                    self.open_file(&path);
                    self.expand_to(&path);
                }
                self.notify(&message, false);
                self.sync_result = Some((message, false));
            }
            result => {
                // Reattach the old workspace after cancelling or rejecting the destination.
                if let Some(root) = self.workspace.clone() {
                    let env = self.env_name().map(str::to_string);
                    self.open_workspace(root, env.as_deref());
                }
                if let Err(error) = result {
                    self.notify(&error, true);
                    self.sync_result = Some((error, true));
                }
            }
        }
    }
}

fn load_account(session: Result<Value, String>) -> AccountResult {
    let session = session?;
    let (email, collections) = auth::account_collections(session.clone())?;
    Ok((session, email, collections))
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::{kittest::Queryable, Harness};

    #[test]
    fn picker_search_login_errors_and_cancelled_results() {
        let ctx = egui::Context::default();
        let mut app = MercuryApp::new(&ctx);
        app.cloud_picker = Some(Picker {
            id: 7,
            pending: false,
            email: Some("example@example.com".into()),
            collections: vec![Collection {
                id: "prj_test".into(),
                name: "Team API".into(),
                root_id: "wrk_test".into(),
                teams: vec![crate::insomnia_sync::Team {
                    name: "Engineering".into(),
                }],
            }],
            search: String::new(),
            error: None,
            challenge: None,
            code: String::new(),
        });
        let mut harness = Harness::builder()
            .with_size(egui::vec2(1000.0, 700.0))
            .build_state(|ctx, app: &mut MercuryApp| app.frame(ctx), app);
        harness.run_steps(2);
        harness.get_by_label("Sign in with browser");
        harness.get_by_label("Use Insomnia login");
        harness.get_by_label("Team API · Engineering");
        harness.state_mut().cloud_picker.as_mut().unwrap().search = "no match".into();
        harness.run_steps(2);
        assert!(harness.query_by_label("Team API · Engineering").is_none());
        harness.get_by_label("No matching collections.");
        harness.state_mut().cloud_picker.as_mut().unwrap().search = "engineering".into();
        harness.run_steps(2);
        harness.get_by_label("Team API · Engineering");

        harness
            .state_mut()
            .on_cloud_account(6, Err("stale result".into()));
        assert!(harness
            .state()
            .cloud_picker
            .as_ref()
            .unwrap()
            .error
            .is_none());
        harness
            .state_mut()
            .on_cloud_account(7, Err("Sign-in expired; sign in again".into()));
        harness.run_steps(2);
        harness.get_by_label("Sign-in expired; sign in again");
        {
            let picker = harness.state_mut().cloud_picker.as_mut().unwrap();
            picker.challenge = Some(auth::LoginChallenge::new().unwrap());
            picker.code = "invalid code".into();
        }
        harness.run_steps(2);
        harness.get_by_label("Complete sign-in").click();
        for _ in 0..100 {
            harness.step();
            if !harness.state().cloud_picker.as_ref().unwrap().pending {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(harness
            .state()
            .cloud_picker
            .as_ref()
            .unwrap()
            .challenge
            .is_some());
        assert_eq!(
            harness
                .state()
                .cloud_picker
                .as_ref()
                .unwrap()
                .error
                .as_deref(),
            Some("Invalid authentication code")
        );
        harness.get_by_label("Close").click();
        harness.run_steps(2);
        assert!(harness.state().cloud_picker.is_none());
        harness
            .state_mut()
            .on_cloud_account(7, Err("closed window".into()));
        assert!(harness.state().cloud_picker.is_none());
    }

    #[test]
    fn cancelled_download_restores_the_watcher_and_keeps_the_open_form() {
        let root = tempfile::tempdir().unwrap();
        let mut app = MercuryApp::new(&egui::Context::default());
        app.open_workspace(root.path().into(), None);
        app.url = "https://example.com/keep".into();
        app.sync_busy = Some("Downloading master…");
        app.watcher = None;
        app.on_cloud_download(Ok(None));
        assert!(app.sync_busy.is_none());
        assert!(app.watcher.is_some());
        assert_eq!(app.url, "https://example.com/keep");
        app.on_cloud_download(Err("Destination has unpushed changes".into()));
        assert!(app.toast.as_ref().unwrap().is_error);
        assert_eq!(app.workspace.as_deref(), Some(root.path()));
    }

    #[test]
    fn downloaded_collection_opens_the_workspace_and_first_request() {
        use std::collections::BTreeMap;
        let root = tempfile::tempdir().unwrap();
        let documents = BTreeMap::from([
            (
                "wrk_test".into(),
                serde_json::json!({"_id":"wrk_test", "type":"Workspace", "name":"Test"}),
            ),
            (
                "req_test".into(),
                serde_json::json!({"_id":"req_test", "type":"Request", "name":"Example", "parentId":"wrk_test", "method":"GET", "url":"https://example.com/master"}),
            ),
        ]);
        let (baseline, warnings) = Baseline::from_master("prj_test", "head", documents).unwrap();
        crate::insomnia_workspace::install_master(root.path(), &baseline, &warnings).unwrap();
        let mut app = MercuryApp::new(&egui::Context::default());
        app.sync_busy = Some("Downloading master…");
        app.on_cloud_download(Ok(Some((root.path().into(), "Fetched master".into()))));
        assert_eq!(app.workspace.as_deref(), Some(root.path()));
        assert_eq!(app.sync_project.as_deref(), Some("prj_test"));
        assert_eq!(app.insomnia_id.as_deref(), Some("req_test"));
        assert_eq!(app.url, "https://example.com/master");
        assert!(app.watcher.is_some());
        assert!(!app.has_unsaved_changes());
    }
}
