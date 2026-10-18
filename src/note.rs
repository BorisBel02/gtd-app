use std::cmp::Ordering;

use chrono::NaiveDate;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum NoteError {
    #[error("invalid note name {0:?}: {1}")]
    InvalidName(String, String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteStatus {
    Todo,
    Done,
}

impl NoteStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            NoteStatus::Todo => "todo",
            NoteStatus::Done => "done",
        }
    }

    pub fn parse(value: &str) -> Option<NoteStatus> {
        match value.trim() {
            "done" => Some(NoteStatus::Done),
            "todo" => Some(NoteStatus::Todo),
            _ => None,
        }
    }
}

/// A note as it lives in memory. `name` is the file name without the `.md`
/// extension; `frontmatter_lines` are the raw YAML lines between the `---`
/// fences (without the fences themselves), or `None` if the file had no
/// frontmatter block.
#[derive(Debug, Clone, PartialEq)]
pub struct Note {
    pub name: String,
    pub deadline: Option<NaiveDate>,
    pub status: NoteStatus,
    pub description: String,
    pub frontmatter_lines: Option<Vec<String>>,
}

impl Note {
    pub fn new(name: String) -> Self {
        Note {
            name,
            deadline: None,
            status: NoteStatus::Todo,
            description: String::new(),
            frontmatter_lines: None,
        }
    }
}

/// Sanitize a user-typed note name into a file-safe base name (without
/// extension).
///
/// - trims surrounding whitespace
/// - removes path separators and control characters
/// - strips a trailing `.md` (case-insensitively, repeatedly) so that typing
///   `report.md` yields the file `report.md`, not `report.md.md`
/// - rejects names starting with a dot (hidden files)
/// - rejects names that are empty after sanitization
pub fn sanitize_name(raw: &str) -> Result<String, NoteError> {
    let cleaned: String = raw
        .trim()
        .chars()
        .filter(|c| !c.is_control() && *c != '/' && *c != '\\')
        .collect();

    let mut name = cleaned;
    while name.len() > 3 && name.to_lowercase().ends_with(".md") {
        name.truncate(name.len() - 3);
    }

    if name.starts_with('.') {
        return Err(NoteError::InvalidName(
            raw.to_string(),
            "name must not start with a dot (hidden files)".into(),
        ));
    }
    if name.is_empty() {
        return Err(NoteError::InvalidName(
            raw.to_string(),
            "name is empty after sanitization".into(),
        ));
    }
    Ok(name)
}

/// Parse a deadline value from a frontmatter line. Tolerates surrounding
/// quotes and ISO datetimes (the date part is kept). Returns `None` for an
/// empty or unparseable value.
pub fn parse_deadline(raw: &str) -> Option<NaiveDate> {
    let value = strip_value(raw)?;
    let date_part = value
        .split(|c| c == 'T' || c == 't' || c == ' ')
        .next()
        .unwrap_or(&value);
    NaiveDate::parse_from_str(date_part, "%Y-%m-%d").ok()
}

/// Turn a raw frontmatter value (the rest of the line after the key) into a
/// clean string: strips surrounding quotes or cuts a trailing ` # comment`.
fn strip_value(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    if let Some(inner) = match raw.chars().next() {
        Some(q @ ('"' | '\'')) => {
            let rest = &raw[1..];
            rest.find(q).map(|end| &rest[..end])
        }
        _ => None,
    } {
        return Some(inner.to_string());
    }
    let cut = raw
        .find(" #")
        .or_else(|| raw.find("\t#"))
        .unwrap_or(raw.len());
    let value = raw[..cut].trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

/// Split file content into (frontmatter lines, body).
///
/// If the first line is exactly `---` and a closing `---`/`...` line exists,
/// the lines between are the frontmatter and everything after the fence is
/// the body. A missing closing fence is tolerated by treating the whole file
/// as body. Line terminators are stripped from frontmatter lines; the body
/// keeps its original bytes.
pub fn split_frontmatter(content: &str) -> (Option<Vec<String>>, &str) {
    let lines: Vec<&str> = content.split_inclusive('\n').collect();
    let Some(first) = lines.first() else {
        return (None, content);
    };
    if first.trim_end_matches(['\r', '\n']) != "---" {
        return (None, content);
    }
    for (i, line) in lines.iter().enumerate().skip(1) {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed == "---" || trimmed == "..." {
            let frontmatter: Vec<String> = lines[1..i]
                .iter()
                .map(|s| s.trim_end_matches(['\r', '\n']).to_string())
                .collect();
            let body_len: usize = lines[i + 1..].iter().map(|s| s.len()).sum();
            let body = &content[content.len() - body_len..];
            return (Some(frontmatter), body);
        }
    }
    (None, content)
}

/// Serialize a note back to file content, preserving every frontmatter line
/// that is not `deadline:`/`status:` byte-for-byte.
pub fn serialize(note: &Note) -> String {
    let mut out = String::new();
    let mut deadline_done = note.deadline.is_none();
    let mut status_done = false;

    match &note.frontmatter_lines {
        Some(lines) => {
            out.push_str("---\n");
            for line in lines {
                let top_level = !line.starts_with(char::is_whitespace);
                if top_level && line.starts_with("deadline:") && !deadline_done {
                    if let Some(d) = note.deadline {
                        out.push_str(&format!("deadline: {}\n", d.format("%Y-%m-%d")));
                    }
                    deadline_done = true;
                } else if top_level && line.starts_with("status:") && !status_done {
                    out.push_str(&format!("status: {}\n", note.status.as_str()));
                    status_done = true;
                } else {
                    out.push_str(line);
                    out.push('\n');
                }
            }
            if !deadline_done {
                if let Some(d) = note.deadline {
                    out.push_str(&format!("deadline: {}\n", d.format("%Y-%m-%d")));
                }
            }
            if !status_done {
                out.push_str(&format!("status: {}\n", note.status.as_str()));
            }
            out.push_str("---\n");
        }
        None => {
            out.push_str("---\n");
            if let Some(d) = note.deadline {
                out.push_str(&format!("deadline: {}\n", d.format("%Y-%m-%d")));
            }
            out.push_str(&format!("status: {}\n", note.status.as_str()));
            out.push_str("---\n");
        }
    }
    out.push_str(&note.description);
    out
}

/// Sort notes by deadline ascending, notes without a deadline last, then by
/// name.
pub fn sort_notes(notes: &mut [Note]) {
    notes.sort_by(|a, b| match (&a.deadline, &b.deadline) {
        (Some(x), Some(y)) => x.cmp(y).then_with(|| a.name.cmp(&b.name)),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => a.name.cmp(&b.name),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate as D;

    #[test]
    fn sanitize_strips_separators_and_control_chars() {
        assert_eq!(sanitize_name("a/b\\c\x00d").unwrap(), "abcd");
    }

    #[test]
    fn sanitize_strips_trailing_md() {
        assert_eq!(sanitize_name("report.md").unwrap(), "report");
        assert_eq!(sanitize_name("report.MD").unwrap(), "report");
        assert_eq!(sanitize_name("report.md.md").unwrap(), "report");
        assert_eq!(sanitize_name("notes.md ").unwrap(), "notes");
    }

    #[test]
    fn sanitize_rejects_leading_dot_and_empty() {
        assert!(sanitize_name(".hidden").is_err());
        assert!(sanitize_name("...").is_err());
        assert!(sanitize_name("").is_err());
        assert!(sanitize_name("///\x00").is_err());
        assert!(sanitize_name(".md").is_err());
    }

    #[test]
    fn sanitize_keeps_plain_names() {
        assert_eq!(sanitize_name("  buy milk  ").unwrap(), "buy milk");
        assert_eq!(sanitize_name("Q4-report_v2").unwrap(), "Q4-report_v2");
    }

    #[test]
    fn deadline_parses_iso_and_datetime_and_quotes() {
        assert_eq!(parse_deadline("2026-12-01"), Some(D::from_ymd_opt(2026, 12, 1).unwrap()));
        assert_eq!(
            parse_deadline("2026-12-01T09:30:00"),
            Some(D::from_ymd_opt(2026, 12, 1).unwrap())
        );
        assert_eq!(
            parse_deadline("\"2026-12-01\""),
            Some(D::from_ymd_opt(2026, 12, 1).unwrap())
        );
        assert_eq!(
            parse_deadline("2026-12-01 14:00"),
            Some(D::from_ymd_opt(2026, 12, 1).unwrap())
        );
        assert_eq!(parse_deadline("2026-12-01 # end of year"), Some(D::from_ymd_opt(2026, 12, 1).unwrap()));
        assert_eq!(parse_deadline(""), None);
        assert_eq!(parse_deadline("someday"), None);
        assert_eq!(parse_deadline("'\"quoted\"'"), None);
    }

    #[test]
    fn frontmatter_split_basic() {
        let content = "---\ndeadline: 2026-12-01\nstatus: todo\n---\nBody text.\n";
        let (fm, body) = split_frontmatter(content);
        assert_eq!(
            fm,
            Some(vec!["deadline: 2026-12-01".to_string(), "status: todo".to_string()])
        );
        assert_eq!(body, "Body text.\n");
    }

    #[test]
    fn frontmatter_absent() {
        let content = "Just a body.\n";
        let (fm, body) = split_frontmatter(content);
        assert!(fm.is_none());
        assert_eq!(body, content);
    }

    #[test]
    fn frontmatter_unclosed_is_tolerated() {
        let content = "---\ndeadline: 2026-12-01\nBody-ish thing.\n";
        let (fm, body) = split_frontmatter(content);
        assert!(fm.is_none());
        assert_eq!(body, content);
    }

    #[test]
    fn frontmatter_closes_with_dotdotdot() {
        let content = "---\ntags: x\n...\nBody.\n";
        let (fm, body) = split_frontmatter(content);
        assert_eq!(fm, Some(vec!["tags: x".to_string()]));
        assert_eq!(body, "Body.\n");
    }

    #[test]
    fn serialize_preserves_unknown_frontmatter_bytes() {
        let mut note = Note::new("report".into());
        note.frontmatter_lines = Some(vec![
            "deadline: 2026-01-01".into(),
            "tags: \"foo, bar\"  # keep me".into(),
            "aliases:".into(),
            "  - old-name".into(),
        ]);
        note.deadline = Some(D::from_ymd_opt(2026, 12, 1).unwrap());
        note.status = NoteStatus::Done;
        note.description = "Body.\n".into();

        let out = serialize(&note);
        assert_eq!(
            out,
            "---\ndeadline: 2026-12-01\ntags: \"foo, bar\"  # keep me\naliases:\n  - old-name\nstatus: done\n---\nBody.\n"
        );
    }

    #[test]
    fn serialize_inserts_missing_fields() {
        let mut note = Note::new("n".into());
        note.frontmatter_lines = Some(vec!["tags: x".into()]);
        note.deadline = Some(D::from_ymd_opt(2026, 5, 5).unwrap());
        let out = serialize(&note);
        assert_eq!(out, "---\ntags: x\ndeadline: 2026-05-05\nstatus: todo\n---\n");
    }

    #[test]
    fn serialize_without_frontmatter() {
        let note = Note::new("n".into());
        assert_eq!(serialize(&note), "---\nstatus: todo\n---\n");
    }

    #[test]
    fn roundtrip_preserves_body_and_extras() {
        let content = "---\ndeadline: 2026-12-01\nstatus: todo\ntags: [a, b]\n---\nLine one\nLine two\n";
        let (fm, body) = split_frontmatter(content);
        let mut note = Note::new("rt".into());
        note.frontmatter_lines = fm;
        note.description = body.to_string();
        note.deadline = parse_deadline("2026-12-01");
        note.status = NoteStatus::Todo;

        let out = serialize(&note);
        assert_eq!(out, content);
    }

    #[test]
    fn sort_deadline_asc_no_deadline_last_then_name() {
        let mut notes = vec![
            Note { name: "zeta".into(), deadline: None, ..Note::new("zeta".into()) },
            Note {
                name: "soon".into(),
                deadline: Some(D::from_ymd_opt(2026, 10, 20).unwrap()),
                ..Note::new("soon".into())
            },
            Note { name: "nomd".into(), deadline: None, ..Note::new("nomd".into()) },
            Note {
                name: "overdue".into(),
                deadline: Some(D::from_ymd_opt(2020, 1, 1).unwrap()),
                ..Note::new("overdue".into())
            },
        ];
        sort_notes(&mut notes);
        let names: Vec<_> = notes.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, ["overdue", "soon", "nomd", "zeta"]);
    }

    #[test]
    fn status_parse() {
        assert_eq!(NoteStatus::parse("done"), Some(NoteStatus::Done));
        assert_eq!(NoteStatus::parse("todo"), Some(NoteStatus::Todo));
        assert_eq!(NoteStatus::parse("wip"), None);
    }
}
