use std::path::{Path, PathBuf};
use std::rc::Rc;

use chrono::{Local, NaiveDate};
use slint::{ModelRc, VecModel, Weak};

use crate::config::Config;
use crate::note::{self, sort_notes, Note, NoteStatus};
use crate::store::Store;
use crate::{MainWindow, Note as UiNote};

pub enum PendingAction {
    Select(String),
    NewNote(String),
    DeleteSelected,
    Refresh,
    ToggleDone(String),
    UseFolder(String),
}

type DetailBaseline = (String, String, String, bool);

pub struct AppState {
    config: Config,
    store: Option<Store>,
    notes: Vec<Note>,
    selected_name: Option<String>,
    detail_baseline: Option<DetailBaseline>,
    pending_action: Option<PendingAction>,
    ui: Weak<MainWindow>,
}

impl AppState {
    pub fn new(config: Config, ui: Weak<MainWindow>) -> Self {
        AppState {
            config,
            store: None,
            notes: Vec::new(),
            selected_name: None,
            detail_baseline: None,
            pending_action: None,
            ui,
        }
    }

    fn ui(&self) -> Option<MainWindow> {
        self.ui.upgrade()
    }

    pub fn startup(&mut self) {
        let Some(ui) = self.ui() else {
            return;
        };
        ui.set_hide_done(self.config.hide_done);
        match self.config.notes_dir.clone() {
            Some(dir) if is_writable_dir(Path::new(&dir)) => {
                self.store = Some(Store::new(PathBuf::from(dir)));
                ui.set_notes_dir_set(true);
                self.refresh();
            }
            Some(dir) => {
                ui.set_initial_folder(dir.clone().into());
                ui.set_onboarding_path(dir.into());
                ui.set_notes_dir_set(false);
                self.set_status("Error: configured notes folder is missing or not writable");
            }
            None => {
                ui.set_notes_dir_set(false);
            }
        }
    }

    /// Run an action, or stash it behind the discard-unsaved-changes prompt
    /// if the detail pane holds edits that differ from the baseline.
    pub fn run_or_defer(&mut self, action: PendingAction) {
        let Some(ui) = self.ui() else {
            return;
        };
        if self.detail_is_dirty(&ui) {
            self.pending_action = Some(action);
            ui.set_discard_confirm_visible(true);
        } else {
            self.exec_action(action);
        }
    }

    fn exec_action(&mut self, action: PendingAction) {
        match action {
            PendingAction::Select(filename) => self.select(filename),
            PendingAction::NewNote(name) => self.new_note(name),
            PendingAction::DeleteSelected => self.delete_selected(),
            PendingAction::Refresh => self.refresh(),
            PendingAction::ToggleDone(filename) => self.toggle_done(filename),
            PendingAction::UseFolder(path) => self.use_folder(path),
        }
    }

    pub fn discard_confirmed(&mut self) {
        let Some(ui) = self.ui() else {
            return;
        };
        ui.set_discard_confirm_visible(false);
        if let Some(action) = self.pending_action.take() {
            self.detail_baseline = None;
            self.exec_action(action);
        }
    }

    pub fn discard_cancelled(&mut self) {
        let Some(ui) = self.ui() else {
            return;
        };
        ui.set_discard_confirm_visible(false);
        self.pending_action = None;
    }

    pub fn toggle_hide_done(&mut self) {
        let Some(ui) = self.ui() else {
            return;
        };
        self.config.hide_done = ui.get_hide_done();
        let _ = self.config.save();
        self.push_model();
    }

    pub fn change_folder(&mut self) {
        let Some(ui) = self.ui() else {
            return;
        };
        ui.set_onboarding_path(ui.get_initial_folder().to_string().into());
        ui.set_notes_dir_set(false);
    }

    pub fn save_detail(&mut self) {
        let Some(ui) = self.ui() else {
            return;
        };
        let Some(store) = self.store.clone() else {
            return;
        };
        let Some(selected_name) = self.selected_name.clone() else {
            return;
        };

        let deadline = match ui.get_detail_deadline().to_string().trim() {
            "" => None,
            text => match NaiveDate::parse_from_str(text, "%Y-%m-%d") {
                Ok(d) => Some(d),
                Err(_) => {
                    self.set_status("Error: deadline must be a date in YYYY-MM-DD format");
                    return;
                }
            },
        };
        let description = ui.get_detail_description().to_string();
        let done = ui.get_detail_done();
        let new_name_raw = ui.get_detail_name().to_string();

        let new_name = match note::sanitize_name(&new_name_raw) {
            Ok(name) if name != selected_name => {
                if self
                    .notes
                    .iter()
                    .any(|n| n.name == name && n.name != selected_name)
                {
                    self.set_status("Error: a note with this name already exists");
                    return;
                }
                name
            }
            Ok(name) => name,
            Err(e) => {
                self.set_status(&format!("Error: {e}"));
                return;
            }
        };

        let renamed = new_name != selected_name;
        let Some(mut note) = self.notes.iter().find(|n| n.name == selected_name).cloned() else {
            self.selected_name = None;
            return;
        };
        note.name = new_name.clone();
        note.deadline = deadline;
        note.description = description;
        note.status = if done {
            NoteStatus::Done
        } else {
            NoteStatus::Todo
        };

        let persist = if renamed {
            store.rename(&selected_name, &note.name).and_then(|()| store.save(&note))
        } else {
            store.save(&note)
        };
        if let Err(e) = persist {
            self.set_status(&format!("Error: {e}"));
            return;
        }

        if let Some(slot) = self.notes.iter_mut().find(|n| n.name == selected_name) {
            *slot = note;
        } else {
            self.notes.push(note);
        }
        sort_notes(&mut self.notes);
        self.selected_name = Some(new_name.clone());
        ui.set_selected(format!("{new_name}.md").into());
        self.push_model();
        self.repopulate_detail();
        self.set_status("Saved");
    }

    pub fn select(&mut self, filename: String) {
        let Some(ui) = self.ui() else {
            return;
        };
        let name = filename.strip_suffix(".md").unwrap_or(&filename).to_string();
        if self.selected_name.as_deref() == Some(name.as_str()) {
            return;
        }
        if !self.notes.iter().any(|n| n.name == name) {
            return;
        }
        self.selected_name = Some(name);
        ui.set_selected(filename.into());
        self.push_model();
        self.repopulate_detail();
    }

    pub fn new_note(&mut self, raw_name: String) {
        let Some(ui) = self.ui() else {
            return;
        };
        let Some(store) = self.store.clone() else {
            return;
        };
        match store.create(&raw_name) {
            Ok(note) => {
                self.reload_notes(&store);
                self.selected_name = Some(note.name.clone());
                ui.set_selected(format!("{}.md", note.name).into());
                self.push_model();
                self.repopulate_detail();
                self.set_status("Note created");
            }
            Err(e) => self.set_status(&format!("Error: {e}")),
        }
    }

    pub fn delete_selected(&mut self) {
        let Some(ui) = self.ui() else {
            return;
        };
        let Some(store) = self.store.clone() else {
            return;
        };
        let Some(selected_name) = self.selected_name.clone() else {
            return;
        };
        if let Err(e) = store.delete(&format!("{selected_name}.md")) {
            self.set_status(&format!("Error: {e}"));
            return;
        }
        self.notes.retain(|n| n.name != selected_name);
        self.selected_name = None;
        ui.set_selected("".into());
        self.push_model();
        self.repopulate_detail();
        self.set_status("Note deleted");
    }

    pub fn refresh(&mut self) {
        let Some(store) = self.store.clone() else {
            return;
        };
        self.reload_notes(&store);
        self.push_model();
        self.repopulate_detail();
    }

    pub fn toggle_done(&mut self, filename: String) {
        let Some(store) = self.store.clone() else {
            return;
        };
        let name = filename.strip_suffix(".md").unwrap_or(&filename).to_string();
        let Some(note) = self.notes.iter_mut().find(|n| n.name == name) else {
            return;
        };
        let previous = note.status;
        note.status = match note.status {
            NoteStatus::Todo => NoteStatus::Done,
            NoteStatus::Done => NoteStatus::Todo,
        };
        if let Err(e) = store.save(note) {
            note.status = previous;
            self.set_status(&format!("Error: {e}"));
            return;
        }
        sort_notes(&mut self.notes);
        self.push_model();
        self.repopulate_detail();
    }

    pub fn use_folder(&mut self, raw_path: String) {
        let Some(ui) = self.ui() else {
            return;
        };
        let dir = PathBuf::from(raw_path.trim());
        if !dir.is_dir() {
            self.set_status("Error: path does not exist or is not a directory");
            return;
        }
        if !is_writable(&dir) {
            self.set_status("Error: folder is not writable");
            return;
        }
        self.store = Some(Store::new(dir.clone()));
        self.config.notes_dir = Some(dir.to_string_lossy().to_string());
        if let Err(e) = self.config.save() {
            self.set_status(&format!("Warning: folder set, but config could not be saved: {e}"));
            return;
        }
        self.notes.clear();
        self.selected_name = None;
        ui.set_initial_folder(dir.to_string_lossy().to_string().into());
        ui.set_notes_dir_set(true);
        self.set_status("Folder loaded");
        self.refresh();
    }

    fn reload_notes(&mut self, store: &Store) {
        match store.list() {
            Ok(mut notes) => {
                sort_notes(&mut notes);
                if self
                    .selected_name
                    .as_ref()
                    .is_some_and(|name| !notes.iter().any(|n| &n.name == name))
                {
                    self.selected_name = None;
                }
                self.notes = notes;
            }
            Err(e) => self.set_status(&format!("Error: {e}")),
        }
    }

    fn push_model(&mut self) {
        let Some(ui) = self.ui() else {
            return;
        };
        let today = Local::now().date_naive();
        let rows: Vec<UiNote> = self
            .notes
            .iter()
            .filter(|n| !self.config.hide_done || n.status != NoteStatus::Done)
            .map(|n| {
                let done = n.status == NoteStatus::Done;
                UiNote {
                    name: n.name.clone().into(),
                    deadline: n
                        .deadline
                        .map(|d| d.format("%Y-%m-%d").to_string().into())
                        .unwrap_or_default(),
                    filename: format!("{}.md", n.name).into(),
                    done,
                    overdue: !done && n.deadline.is_some_and(|d| d < today),
                }
            })
            .collect();
        ui.set_notes(ModelRc::from(Rc::new(VecModel::from(rows))));
    }

    fn repopulate_detail(&mut self) {
        let Some(ui) = self.ui() else {
            return;
        };
        let note = self.selected_name.as_ref().and_then(|name| {
            self.notes
                .iter()
                .find(|n| &n.name == name)
                .map(|n| (
                    n.name.clone(),
                    n.deadline
                        .map(|d| d.format("%Y-%m-%d").to_string())
                        .unwrap_or_default(),
                    n.description.clone(),
                    n.status == NoteStatus::Done,
                ))
        });
        match note {
            Some(values) => {
                ui.set_detail_name(values.0.clone().into());
                ui.set_detail_deadline(values.1.clone().into());
                ui.set_detail_description(values.2.clone().into());
                ui.set_detail_done(values.3);
                ui.set_detail_editable(true);
                self.detail_baseline = Some(values);
            }
            None => {
                ui.set_detail_name("".into());
                ui.set_detail_deadline("".into());
                ui.set_detail_description("".into());
                ui.set_detail_done(false);
                ui.set_detail_editable(false);
                self.detail_baseline = None;
            }
        }
    }

    fn detail_is_dirty(&self, ui: &MainWindow) -> bool {
        match &self.detail_baseline {
            Some((name, deadline, description, done)) => {
                ui.get_detail_name().to_string() != *name
                    || ui.get_detail_deadline().to_string() != *deadline
                    || ui.get_detail_description().to_string() != *description
                    || ui.get_detail_done() != *done
            }
            None => false,
        }
    }

    fn set_status(&mut self, message: &str) {
        if let Some(ui) = self.ui() {
            ui.set_status_message(message.into());
        }
    }
}

fn is_writable_dir(path: &Path) -> bool {
    path.is_dir() && is_writable(path)
}

fn is_writable(dir: &Path) -> bool {
    let probe = dir.join(".gtd-write-probe");
    match std::fs::File::create(&probe) {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}
