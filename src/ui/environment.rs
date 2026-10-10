//! Edit the environment file itself, keeping request and session variables separate.

use super::VenusApp;
use crate::insomnia_workspace::{atomic_write, Baseline, STATE};
use crate::kv;
use crate::ui::{theme::Space, widgets};
use eframe::egui::{ScrollArea, Ui};
use std::{fs, path::PathBuf};

pub(super) struct EnvironmentEditor {
    path: PathBuf,
    original: String,
    text: String,
    json: bool,
    bulk: bool,
}

impl EnvironmentEditor {
    fn open(path: PathBuf, json: bool) -> Result<Self, String> {
        let original = fs::read_to_string(&path).map_err(|e| e.to_string())?;
        let text = if json {
            let values: serde_json::Map<String, serde_json::Value> =
                serde_json::from_str(&original).map_err(|e| e.to_string())?;
            if values.keys().any(|key| {
                key.trim() != key
                    || key.is_empty()
                    || key.contains(['=', '\n', '\r'])
                    || key.starts_with('#')
            }) {
                return Err("This environment has keys that cannot be represented in the key/value editor".into());
            }
            values
                .iter()
                .map(|(key, value)| format!("{key}={value}"))
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            original.clone()
        };
        Ok(Self {
            path,
            original,
            text,
            json,
            bulk: false,
        })
    }

    fn save(&self) -> Result<(), String> {
        if fs::read_to_string(&self.path).map_err(|e| e.to_string())? != self.original {
            return Err("Environment changed on disk. Cancel and reopen it before saving.".into());
        }
        let content = if self.json {
            let mut values = serde_json::Map::new();
            for row in kv::parse_lines(&self.text, "=")
                .into_iter()
                .filter(|row| row.enabled)
            {
                if row.key.is_empty() || values.contains_key(&row.key) {
                    return Err("Environment keys must be nonempty and unique".into());
                }
                let value = serde_json::from_str::<serde_json::Value>(&row.value)
                    .map_err(|e| format!("{} needs a JSON value: {e}", row.key))?;
                values.insert(row.key, value);
            }
            serde_json::to_string_pretty(&values).map_err(|e| e.to_string())?
        } else {
            let mut lines = Vec::new();
            for line in self.text.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed.starts_with('#') {
                    lines.push(line.to_string());
                    continue;
                }
                let key = trimmed
                    .split_once('=')
                    .map_or(trimmed, |(key, _)| key)
                    .trim();
                if key.is_empty() {
                    return Err("Environment keys cannot be empty".into());
                }
                // The shared table omits the separator for empty values.
                lines.push(if line.contains('=') {
                    line.to_string()
                } else {
                    format!("{line}=")
                });
            }
            lines.join("\n")
        };
        atomic_write(&self.path, content.as_bytes())
    }
}

impl VenusApp {
    pub(super) fn open_environment_editor(&mut self, index: usize) {
        if self.environment_editor.is_some() {
            self.notify("Save or cancel the open environment editor first", true);
            return;
        }
        let Some((root, name)) = self.workspace.as_ref().zip(self.env_files.get(index)) else {
            return;
        };
        let json = if root.join(STATE).exists() {
            match Baseline::load(root) {
                Ok(baseline) => baseline.environments.values().any(|path| path == name),
                Err(error) => {
                    self.notify(error, true);
                    return;
                }
            }
        } else {
            false
        };
        match EnvironmentEditor::open(root.join(name), json) {
            Ok(editor) => {
                self.select_env(Some(index));
                self.environment_editor = Some(editor);
            }
            Err(error) => self.notify(format!("Could not open environment: {error}"), true),
        }
    }

    pub(in crate::ui) fn environment_pane(&mut self, ui: &mut Ui) -> bool {
        let Some(editor) = self.environment_editor.as_mut() else {
            return false;
        };
        let mut save = false;
        let mut cancel = false;
        widgets::card().show(ui, |ui| {
            ui.set_min_height(ui.available_height());
            ui.horizontal(|ui| {
                ui.heading(format!(
                    "Environment · {}",
                    editor.path.file_name().unwrap_or_default().to_string_lossy()
                ));
                save = ui.button("Save environment").clicked();
                cancel = ui.button("Cancel").clicked();
            });
            if editor.json {
                ui.label(widgets::muted("Values use JSON: \"text\", 42, true, or an object. Disabled rows are removed on save."));
            }
            ui.add_space(Space::MD);
            ScrollArea::vertical().show(ui, |ui| {
                widgets::key_value_editor(
                    ui,
                    &mut editor.text,
                    "=",
                    &mut editor.bulk,
                    "BASE=https://api.example.com\nTOKEN=secret",
                );
            });
        });
        if cancel {
            self.environment_editor = None;
        } else if save {
            self.save_environment_editor();
        }
        true
    }

    pub(super) fn save_environment_editor(&mut self) {
        let Some(editor) = &self.environment_editor else {
            return;
        };
        match editor.save() {
            Ok(()) => {
                self.environment_editor = None;
                self.load_env();
                self.notify("Environment saved", false);
            }
            Err(error) => self.notify(error, true),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dotenv_save_keeps_comments_and_empty_values_and_detects_conflicts() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".env");
        fs::write(&path, "# comment\nTOKEN=old\n").unwrap();
        let mut editor = EnvironmentEditor::open(path.clone(), false).unwrap();
        editor.text = "# comment\nTOKEN=new\nEMPTY".into();
        editor.save().unwrap();
        assert_eq!(crate::vars::parse_env(&fs::read_to_string(&path).unwrap())["EMPTY"], "");
        assert!(fs::read_to_string(&path).unwrap().starts_with("# comment\nTOKEN=new"));
        editor.text = "TOKEN=stale".into();
        assert!(editor.save().unwrap_err().contains("changed on disk"));
        assert_eq!(crate::vars::parse_env(&fs::read_to_string(&path).unwrap())["TOKEN"], "new");
    }

    #[test]
    fn json_save_preserves_types_and_rejects_invalid_edits() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".env.cloud");
        let original = r#"{"TOKEN":"old","COUNT":42,"OPTIONS":{"enabled":true}}"#;
        fs::write(&path, original).unwrap();
        let mut editor = EnvironmentEditor::open(path.clone(), true).unwrap();
        editor.text = editor.text.replace("TOKEN=\"old\"", "TOKEN=\"new\"");
        editor.save().unwrap();
        let values: serde_json::Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(values["TOKEN"], "new");
        assert_eq!(values["COUNT"], 42);
        assert_eq!(values["OPTIONS"]["enabled"], true);
        let mut editor = EnvironmentEditor::open(path.clone(), true).unwrap();
        editor.text = "TOKEN=invalid".into();
        assert!(editor.save().is_err());
        let unchanged = fs::read_to_string(&path).unwrap();
        editor.text = "TOKEN=1\nTOKEN=2".into();
        assert!(editor.save().is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), unchanged);
    }
}
