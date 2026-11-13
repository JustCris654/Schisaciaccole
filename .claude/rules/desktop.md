---
paths:
  - "crates/app/**"
---

# Desktop app

Read `README.org`, the affected Rust module and Slint UI before editing. Keep domain calculations in core and UI orchestration in the app. The default build is local-only; the optional backend URL is fixed at compile time.

Preserve Manuale, meeting/turn/Daily navigation, timer completion/stop return paths, compact window size and restoration, always-on-top behavior, shortcuts and sound resources. The desktop does not use server slot templates/plans; do not infer an editor from those endpoints.

Keep blocking network/keychain work off UI callbacks using the established worker patterns. Preserve browser/loopback PKCE login, OS keychain refresh tokens and logout handling. Follow existing Slint styling and bundle resource paths. Package commands run from `crates/app`, not the workspace root; see `ci-versioning.md`.
