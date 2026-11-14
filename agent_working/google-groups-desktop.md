# Google Groups desktop progress

Own crates/app/** and README.md only. Other agents own core/server; preserve their changes and the previous meeting editor commit. No commits here.

Contract coordinated with core/server: Meeting and SlotPlan ParticipantResolution flags; SlotPlan optional fresh attendees and attendees_omitted. Editor adopts fresh attendee snapshot so equal reset and subsequent meeting list/Avvia/Daily share the latest read. Legacy plans keep existing participants and conservatively merge warnings.

Implemented initial row/editor incomplete notices for Calendar omission or group fallback, neutral participant wording, explicit inline reconnect using existing worker login with cancel and status while Calendar stays usable. No periodic refresh starts login. Reconnect success explicitly refreshes meetings; failure/cancel preserve signed-in meetings and editor.

Validation complete: `cargo test -p schisaciaccole` passed all 249 tests with approved loopback socket access (sandbox-only run previously blocked 22 existing network tests). `cargo clippy -p schisaciaccole --all-targets -- -D warnings` passed, including Slint UI compilation. New coverage adds 10 state tests and 2 editor tests for warnings, fresh participant resets/list/Avvia/Daily, neutral counts, reconnect/cancel/failure and no automatic login loops. `git diff --check -- crates/app README.md` passed. App Rust files formatted with rustfmt. README updated with verified Google scope/permissions links and implementation behavior/limits/manual check instructions. No real invitations created.

Awaiting orchestrator review and workspace validation. No commit made. Glossaries intentionally removed per user. External document session access unavailable as verified by orchestrator; no external update claimed.
