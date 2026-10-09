//! Review saved collection changes before publishing a named cloud commit.

use super::*;

pub(super) struct PushDialog {
    pub message: String,
    pub fingerprint: Option<String>,
    pub error: Option<String>,
    summary: String,
    recovery: bool,
}

impl VenusApp {
    pub(super) fn open_push_dialog(&mut self) {
        if self.sync_busy.is_some() {
            return;
        }
        let (Some(root), Some(project)) = (self.workspace.clone(), self.sync_project.clone())
        else {
            return;
        };
        if self.has_unsaved_changes() && !self.save_file() {
            return;
        }
        let message = self
            .push_dialog
            .take()
            .map(|dialog| dialog.message)
            .unwrap_or_default();
        self.push_dialog = Some(PushDialog {
            message,
            fingerprint: None,
            error: None,
            summary: String::new(),
            recovery: root.join(crate::insomnia_sync::PENDING).exists(),
        });
        self.sync_busy = Some("Reviewing changes on master…");
        self.watcher = None;
        self.spawn(move || {
            let result = (|| {
                let before = crate::insomnia_workspace::push_fingerprint(&root)?;
                let args = vec![
                    "--insomnia-plan".into(),
                    project,
                    root.to_string_lossy().into_owned(),
                ];
                let summary = crate::insomnia_sync::run_cli(&args).unwrap()?;
                if before != crate::insomnia_workspace::push_fingerprint(&root)? {
                    return Err(
                        "Collection changed while preparing the preview; review again".into(),
                    );
                }
                Ok((summary, before))
            })();
            Some(Event::PushPrepared(root, result))
        });
    }

    pub(super) fn on_push_prepared(
        &mut self,
        root: PathBuf,
        result: Result<(String, String), String>,
    ) {
        let dialog = self.push_dialog.take();
        self.finish_sync(
            root.clone(),
            match &result {
                Ok(_) => Ok("Collection changes ready to review".into()),
                Err(error) => Err(error.clone()),
            },
        );
        if self.workspace.as_ref() != Some(&root) {
            return;
        }
        self.push_dialog = dialog.map(|mut dialog| {
            match result {
                Ok((summary, fingerprint)) => {
                    if summary.starts_with("Master push plan (") {
                        dialog.fingerprint = Some(fingerprint);
                    }
                    dialog.summary = summary;
                }
                Err(error) => dialog.error = Some(error),
            }
            dialog
        });
    }

    pub(super) fn show_push_dialog(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.push_dialog.as_mut() else {
            return;
        };
        let mut review = false;
        let mut push = false;
        let action = modal(ctx, "Push collection to master", |ui| {
            ui.label(muted(
                "All saved request and environment changes will be included.",
            ));
            ui.add_space(Space::MD);
            if !dialog.summary.is_empty() {
                egui::ScrollArea::vertical()
                    .max_height(180.0)
                    .show(ui, |ui| {
                        for line in dialog.summary.lines() {
                            let label = line
                                .strip_suffix(')')
                                .and_then(|line| line.rsplit_once(" ("))
                                .filter(|(_, id)| {
                                    ["req_", "env_", "fld_"]
                                        .iter()
                                        .any(|prefix| id.starts_with(prefix))
                                })
                                .map(|(label, _)| label)
                                .unwrap_or(line);
                            ui.label(label);
                        }
                    });
            }
            if let Some(error) = &dialog.error {
                ui.label(egui::RichText::new(error).color(theme().error));
            }
            ui.add_space(Space::MD);
            if dialog.recovery {
                ui.label(
                    "Resume the interrupted push using its original message and saved changes.",
                );
            } else {
                ui.label(strong("Commit message"));
                ui.add(
                    egui::TextEdit::multiline(&mut dialog.message)
                        .hint_text("Describe your changes\n\nOptional details")
                        .desired_rows(4)
                        .desired_width(f32::INFINITY),
                );
                if dialog.message.len() > 4096 {
                    ui.label(
                        egui::RichText::new("Commit message must be at most 4096 bytes.")
                            .color(theme().error),
                    );
                }
            }
            ui.add_space(Space::MD);
            let mut close = false;
            ui.horizontal(|ui| {
                close = ui.button("Cancel").clicked();
                review = ui.button("Review again").clicked();
                let enabled = dialog.recovery
                    || (dialog.fingerprint.is_some()
                        && crate::insomnia_sync::commit_message(&dialog.message).is_ok());
                push = ui
                    .add_enabled(
                        enabled,
                        egui::Button::new(if dialog.recovery {
                            "Resume previous push"
                        } else {
                            "Push to master"
                        }),
                    )
                    .clicked();
            });
            if close {
                ModalAction::Close
            } else {
                ModalAction::Open
            }
        });
        if action == ModalAction::Close {
            self.push_dialog = None;
        } else if review {
            self.open_push_dialog();
        } else if push {
            if self
                .push_dialog
                .as_ref()
                .is_some_and(|dialog| dialog.recovery)
            {
                self.push_dialog.as_mut().unwrap().message = "Resume interrupted push".into();
            }
            self.start_sync(false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::{
        kittest::{NodeT, Queryable},
        Harness,
    };

    #[test]
    fn push_dialog_requires_message_and_preserves_draft_on_error() {
        let ctx = egui::Context::default();
        let mut app = VenusApp::new(&ctx);
        app.push_dialog = Some(PushDialog {
            message: String::new(), fingerprint: Some("preview".into()), error: None,
            summary: "Master push plan (2 resources):\nUpdate Request: PDF\nUpdate Environment: Development".into(),
            recovery: false,
        });
        let mut harness = Harness::builder()
            .with_size(egui::vec2(1000.0, 700.0))
            .build_state(|ctx, app: &mut VenusApp| app.frame(ctx), app);
        harness.run_steps(2);
        assert!(harness
            .get_by_role_and_label(egui::accesskit::Role::Button, "Push to master")
            .accesskit_node()
            .is_disabled());
        harness.state_mut().push_dialog.as_mut().unwrap().message =
            "Update PDF and development host".into();
        harness.run_steps(2);
        assert!(!harness
            .get_by_role_and_label(egui::accesskit::Role::Button, "Push to master")
            .accesskit_node()
            .is_disabled());
        let root = tempfile::tempdir().unwrap();
        harness
            .state_mut()
            .finish_sync(root.path().into(), Err("Connection interrupted".into()));
        harness.run_steps(2);
        assert_eq!(
            harness.state().push_dialog.as_ref().unwrap().message,
            "Update PDF and development host"
        );
        assert!(harness
            .get_by_role_and_label(egui::accesskit::Role::Button, "Push to master")
            .accesskit_node()
            .is_disabled());
        harness.get_by_label("Cancel").click();
        harness.run_steps(2);
        assert!(harness.state().push_dialog.is_none());
    }

    #[test]
    fn empty_preview_disables_push_and_recovery_can_resume() {
        let ctx = egui::Context::default();
        let mut app = VenusApp::new(&ctx);
        app.push_dialog = Some(PushDialog {
            message: "Saved message".into(),
            fingerprint: None,
            error: None,
            summary: "No local request or environment changes to push".into(),
            recovery: false,
        });
        let mut harness = Harness::builder()
            .with_size(egui::vec2(1000.0, 700.0))
            .build_state(|ctx, app: &mut VenusApp| app.frame(ctx), app);
        harness.run_steps(2);
        assert!(harness
            .get_by_role_and_label(egui::accesskit::Role::Button, "Push to master")
            .accesskit_node()
            .is_disabled());
        harness.state_mut().push_dialog.as_mut().unwrap().recovery = true;
        harness.run_steps(2);
        assert!(!harness
            .get_by_role_and_label(egui::accesskit::Role::Button, "Resume previous push")
            .accesskit_node()
            .is_disabled());
    }
}
