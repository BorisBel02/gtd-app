use std::fs;
use std::path::{Component, Path, PathBuf};

use thiserror::Error;

use crate::note::{self, Note, NoteError, NoteStatus};

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    InvalidName(#[from] NoteError),
    #[error("a note named {0:?} already exists")]
    AlreadyExists(String),
    #[error("invalid filename {0:?}")]
    InvalidFilename(String),
}

/// Filesystem-backed store: one Markdown file per note, directly inside
/// `notes_dir` (non-recursive).
#[derive(Debug, Clone)]
pub struct Store {
    notes_dir: PathBuf,
}

impl Store {
    pub fn new(notes_dir: PathBuf) -> Self {
        Store { notes_dir }
    }

    /// Join `filename` into the notes dir, refusing to escape it. The
    /// filename must be a single relative component (no `..`, no absolute
    /// paths, no separators).
    fn path_for(&self, filename: &str) -> Result<PathBuf, StoreError> {
        let p = Path::new(filename);
        if filename.is_empty()
            || p.is_absolute()
            || p.components().count() != 1
            || !matches!(p.components().next(), Some(Component::Normal(_)))
        {
            return Err(StoreError::InvalidFilename(filename.to_string()));
        }
        Ok(self.notes_dir.join(p))
    }

    /// Read and parse every `*.md` file directly in the notes folder.
    /// Unreadable or malformed files are skipped with a warning rather than
    /// failing the whole list.
    pub fn list(&self) -> Result<Vec<Note>, StoreError> {
        let mut notes = Vec::new();
        for entry in fs::read_dir(&self.notes_dir)? {
            let entry = entry?;
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
                continue;
            };
            if !ext.eq_ignore_ascii_case("md") {
                continue;
            }
            let Ok(content) = fs::read_to_string(&path) else {
                eprintln!("gtd-app: skipping unreadable note {name}");
                continue;
            };
            let Some(note) = Self::parse_note_content(name, &content) else {
                eprintln!("gtd-app: skipping malformed note {name}");
                continue;
            };
            notes.push(note);
        }
        Ok(notes)
    }

    fn parse_note_content(filename: &str, content: &str) -> Option<Note> {
        let stem = if filename.to_lowercase().ends_with(".md") {
            &filename[..filename.len() - 3]
        } else {
            return None;
        };
        let (frontmatter_lines, body) = note::split_frontmatter(content);

        let mut deadline = None;
        let mut status = NoteStatus::Todo;
        if let Some(lines) = &frontmatter_lines {
            for line in lines {
                if line.starts_with(char::is_whitespace) {
                    continue;
                }
                if let Some(value) = line.strip_prefix("deadline:") {
                    deadline = note::parse_deadline(value);
                } else if let Some(value) = line.strip_prefix("status:") {
                    if let Some(s) = NoteStatus::parse(value) {
                        status = s;
                    }
                }
            }
        }

        Some(Note {
            name: stem.to_string(),
            deadline,
            status,
            description: body.to_string(),
            frontmatter_lines,
        })
    }

    /// Create a new note with the given (sanitized) name. Fails if a note
    /// with that name already exists.
    pub fn create(&self, raw_name: &str) -> Result<Note, StoreError> {
        let name = note::sanitize_name(raw_name)?;
        let target = self.path_for(&format!("{name}.md"))?;
        if target.exists() {
            return Err(StoreError::AlreadyExists(name));
        }
        let note = Note::new(name);
        self.save(&note)?;
        Ok(note)
    }

    /// Rename a note's file. Fails if the target name already exists.
    pub fn rename(&self, old: &str, new: &str) -> Result<(), StoreError> {
        let old_name = note::sanitize_name(old)?;
        let new_name = note::sanitize_name(new)?;
        let old_file = self.path_for(&format!("{old_name}.md"))?;
        let new_file = self.path_for(&format!("{new_name}.md"))?;
        if new_file.exists() {
            return Err(StoreError::AlreadyExists(new_name));
        }
        fs::rename(&old_file, &new_file)?;
        Ok(())
    }

    pub fn delete(&self, filename: &str) -> Result<(), StoreError> {
        if !filename.ends_with(".md") {
            return Err(StoreError::InvalidFilename(filename.to_string()));
        }
        let target = self.path_for(filename)?;
        fs::remove_file(target)?;
        Ok(())
    }

    /// Atomically write a note to `<name>.md`: write to `<name>.md.tmp`
    /// first, then rename over the target. On error the tmp file is removed
    /// so a crash can never leave a torn note file behind.
    pub fn save(&self, note: &Note) -> Result<(), StoreError> {
        let filename = format!("{}.md", note.name);
        let target = self.path_for(&filename)?;
        let tmp = self.notes_dir.join(format!("{filename}.tmp"));
        let content = note::serialize(note);
        if let Err(e) = fs::write(&tmp, content) {
            let _ = fs::remove_file(&tmp);
            return Err(StoreError::Io(e));
        }
        if let Err(e) = fs::rename(&tmp, &target) {
            let _ = fs::remove_file(&tmp);
            return Err(StoreError::Io(e));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate as D;

    fn tmp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("gtd-app-test-{}-{}", tag, std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn note(name: &str, deadline: Option<D>, status: NoteStatus, body: &str) -> Note {
        let mut n = Note::new(name.to_string());
        n.deadline = deadline;
        n.status = status;
        n.description = body.to_string();
        n
    }

    #[test]
    fn save_parse_roundtrip() {
        let dir = tmp_dir("roundtrip");
        let store = Store::new(dir.clone());
        let n = note("report", Some(D::from_ymd_opt(2026, 12, 1).unwrap()), NoteStatus::Todo, "Write it.\n");
        store.save(&n).unwrap();

        let listed = store.list().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "report");
        assert_eq!(listed[0].deadline, Some(D::from_ymd_opt(2026, 12, 1).unwrap()));
        assert_eq!(listed[0].status, NoteStatus::Todo);
        assert_eq!(listed[0].description, "Write it.\n");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn unknown_frontmatter_preserved_byte_for_byte() {
        let dir = tmp_dir("preserved");
        let store = Store::new(dir.clone());
        let n = note("interop", Some(D::from_ymd_opt(2026, 3, 1).unwrap()), NoteStatus::Todo, "Body.\n");
        store.save(&n).unwrap();

        // Simulate an Obsidian user adding a property and a comment.
        let path = dir.join("interop.md");
        let content = fs::read_to_string(&path).unwrap();
        let edited = content.replace("status: todo\n", "status: todo\ntags: \"gtd, work\"   # from obsidian\n");
        fs::write(&path, edited).unwrap();

        let mut listed = store.list().unwrap();
        assert_eq!(listed.len(), 1);
        listed[0].deadline = Some(D::from_ymd_opt(2026, 4, 2).unwrap());
        store.save(&listed[0]).unwrap();

        let out = fs::read_to_string(&path).unwrap();
        assert!(
            out.contains("tags: \"gtd, work\"   # from obsidian\n"),
            "custom property line must be preserved, got:\n{out}"
        );
        assert!(out.contains("deadline: 2026-04-02"), "deadline must be updated, got:\n{out}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn quoted_and_datetime_deadlines_tolerated_on_read() {
        let dir = tmp_dir("tolerant");
        let store = Store::new(dir.clone());
        fs::write(
            dir.join("a.md"),
            "---\ndeadline: \"2026-06-15\"\nstatus: done\n---\nA\n",
        )
        .unwrap();
        fs::write(
            dir.join("b.md"),
            "---\ndeadline: 2026-06-16T10:00:00\n---\nB\n",
        )
        .unwrap();

        let mut listed = store.list().unwrap();
        listed.sort_by(|a, b| a.name.cmp(&b.name));
        assert_eq!(listed[0].name, "a");
        assert_eq!(listed[0].deadline, Some(D::from_ymd_opt(2026, 6, 15).unwrap()));
        assert_eq!(listed[0].status, NoteStatus::Done);
        assert_eq!(listed[1].name, "b");
        assert_eq!(listed[1].deadline, Some(D::from_ymd_opt(2026, 6, 16).unwrap()));
        assert_eq!(listed[1].status, NoteStatus::Todo);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn unclosed_frontmatter_is_tolerated() {
        let dir = tmp_dir("unclosed");
        let store = Store::new(dir.clone());
        fs::write(dir.join("x.md"), "---\ndeadline: 2026-01-01\nthis whole file is body\n").unwrap();

        let listed = store.list().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].deadline, None);
        assert!(listed[0].description.contains("this whole file is body"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn create_sanitizes_and_rejects_duplicates() {
        let dir = tmp_dir("create");
        let store = Store::new(dir.clone());
        let created = store.create("report.md").unwrap();
        assert_eq!(created.name, "report");
        assert!(dir.join("report.md").exists());
        assert!(!dir.join("report.md.md").exists());
        assert!(matches!(store.create("report"), Err(StoreError::AlreadyExists(_))));
        assert!(store.create(".hidden").is_err());
        assert_eq!(store.create("a/b").unwrap().name, "ab");
        assert!(store.create("   ").is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rename_and_delete() {
        let dir = tmp_dir("rename");
        let store = Store::new(dir.clone());
        store.create("old").unwrap();
        store.rename("old", "new name").unwrap();
        assert!(!dir.join("old.md").exists());
        assert!(dir.join("new name.md").exists());

        // Collision: create a second note, then try to rename onto it.
        store.create("target").unwrap();
        assert!(matches!(store.rename("new name", "target"), Err(StoreError::AlreadyExists(_))));

        store.delete("target.md").unwrap();
        assert!(!dir.join("target.md").exists());
        assert!(store.delete("../escape.md").is_err());
        assert!(store.delete("nope.md").is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn list_ignores_non_md_and_subdirs() {
        let dir = tmp_dir("listfilter");
        let store = Store::new(dir.clone());
        fs::write(dir.join("note.md"), "body\n").unwrap();
        fs::write(dir.join("readme.txt"), "not a note\n").unwrap();
        fs::create_dir(dir.join("sub")).unwrap();
        fs::write(dir.join("sub").join("inner.md"), "nested\n").unwrap();

        let listed = store.list().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "note");
        let _ = fs::remove_dir_all(&dir);
    }
}
