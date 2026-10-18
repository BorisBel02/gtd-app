# gtd-app

A small GTD notes app built with [Slint](https://slint.rs/). Each note is one
Markdown file (`.md`) in a folder you choose — the files are
[Obsidian](https://obsidian.md)-compatible, so the same folder can be a
subfolder of your vault.

## Note format

```markdown
---
deadline: 2026-12-01
status: todo
---
Body of the note.
```

- `deadline`: ISO date, optional (absent = no deadline)
- `status`: `todo` (default) or `done`
- Everything else in the frontmatter and body is preserved byte-for-byte, so
  properties added in Obsidian survive round-trips.

Notes are sorted by deadline (earliest first, no deadline last); overdue,
unfinished notes are highlighted.

## Build & run

```sh
cargo build
cargo run
```

On first launch the app asks for the path to your notes folder. The choice is
stored in `~/.config/gtd-app/config.toml` (or `$XDG_CONFIG_HOME/gtd-app/`).
If the configured folder is missing or not writable on a later start, the
app re-shows the folder prompt with the old path prefilled.

## Tests

```sh
cargo test
```

Covers name sanitization, frontmatter parsing/serialization (including
byte-for-byte preservation of unknown lines), store CRUD, atomic writes,
path confinement, and config handling.
