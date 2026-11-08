---
paths:
  - "crates/**"
  - "deploy/**"
  - "README.org"
  - "glossary/**"
  - ".env.example"
  - "Dockerfile*"
---

# Docs (keep in sync)

Read this when a change alters what a tab describes: a feature or behavior, a route, an env var, a config key, a Docker Compose service or volume, a workflow, a deploy step, or an operational procedure. Skip it for work that does not change those.

Team doc: https://claude.ai/artifact/RDuHSqU5U1iojNtVAXRRa9?sk=0ssAImgbKiZees8zWqBSPQ#a4567e9d-b41c.mn7zbnxfg4c.18766
Read and edit it with the docs tools.

**Keep the whole doc current, not only the glossaries.** Reflect the change in every affected tab in the same session, before the task is reported done. Read the tab outline first. Edit only the affected blocks in place, in the doc's existing style and language. People edit the doc too: never overwrite their text. If no tab fits, add a section to the closest one or ask the developer. The final message names the tabs updated.

**Rust glossaries.** Each `Glossario` sub-tab has one `## <file>` table per source file (`Nome | Tipo | Riga | Descrizione`) listing every fn, method, struct, enum, trait, type alias, const, macro, and trait impl. Excluded: tests, inherent `impl` blocks (methods are listed), code generated inside `quote!`. When a change adds, removes, renames, or moves a Rust item in a documented crate — line shifts from edits above an included item count — update the affected rows and the commit named in the sub-tab lead. Check every `Riga` (`sed -n '<line>p' <file>`). A new Rust crate gets its own `Glossario` sub-tab, same format.
