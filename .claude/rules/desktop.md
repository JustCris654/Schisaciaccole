---
paths:
  - "crates/app/**"
---

# Desktop app

Read `README.org`, the affected Rust module and Slint UI before editing. Keep domain calculations in core and UI orchestration in the app. The default build is local-only; the optional backend URL is fixed at compile time.

Preserve Manuale, meeting/turn/Daily navigation, timer completion/stop return paths, compact window size and restoration, always-on-top behavior, shortcuts and sound resources. The participant editor opens locally on a per-event draft built from the meetings list (Calendar attendees plus the embedded meeting preference); it makes no request on open. Only preference save/delete use the backend (`preference-save`/`preference-delete` workers), and they update the listed meetings locally without refetching.

Keep blocking network/keychain work off UI callbacks using the established worker patterns. Preserve browser/loopback PKCE login, OS keychain refresh tokens and logout handling. Follow existing Slint styling and bundle resource paths. Package commands run from `crates/app`, not the workspace root; see `ci-versioning.md`.
