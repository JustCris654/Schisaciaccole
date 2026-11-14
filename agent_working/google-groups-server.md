# Google Groups server implementation

Ownership: crates/server/**, .env.example. Preserve simultaneous core/app edits. No commits.

Implementing bounded request-local Cloud Identity lookup and membership traversal, common list/plan pipeline, explicit grant metadata, and mock coverage. Core contract agreed: ParticipantResolution; GroupExpansion and merge_group_attendees. Fresh plan includes Some(attendees), attendees_omitted.

Confirmed Cloud Identity official REST endpoints and preferredMemberKey/type schema. Lookup 404 conservatively retains original attendee and marks incomplete, per developer choice. Membership USER is sufficient proof of a person; GROUP and unknown types require lookup.

Resume progress: fixed Scope accessor, unit GoogleDenied construction and borrowed mock base URLs; server library test binary now compiles. Provider token errors now use opaque static summaries. Effective stored grants are reread after access_token (within the expansion deadline), and initial identity/grant reads also share that deadline. Existing staged leaf cap is covered explicitly for cached membership reuse.

Review-driven concurrency fix in progress: serialize refresh credential read/HTTP/persist/cache and successful reconnect persistence/cache invalidation using a transaction-scoped per-user PostgreSQL advisory lock. Credential helpers accept either pool or transaction executor so no additional pool connection is acquired while holding the lock. Query is runtime-bound, no schema change or modified SQLx macro query text; orchestrator will run migration/prepare verification. Tests for delayed refresh versus reconnect and omitted/reduced grants are being added. No commits.

Implementation frozen after resume validation. Added `groups.rs` resolver + `tests/groups.rs` route coverage; updated common list/plan expansion, fresh plan attendees, explicit grant handling and Cloud Identity base URL. Group lookup/membership are GET-only/FULL-paginated; all provider details remain opaque. Limits cover request/depth/pages/discovered records/staged leaves/body bytes and shared/request timeouts. 404 conservatively retains original invitation and marks incomplete.

Concurrency review resolved with the per-user transaction advisory lock around both credential refresh and reconnect persistence; all credential reads/writes use that transaction executor. Regression verifies a delayed old refresh cannot overwrite reconnect credentials or repopulate its stale token after reconnect. This is a process-local access-token cache; no claim of cross-process cache invalidation.

Validation (temporary PostgreSQL localhost:55439, escalated only for local sockets, SQLX_OFFLINE=true): server lib 253/253, auth_flow 31/31, route groups 5/5 passed before last 4 unit additions; final resolver suite 17/17 passed including user-isolated request caches, real oversized record page, malformed/oversized payload, two-second per-request timeout, and cached staged-leaf cap. Final strict server clippy all-targets passed. `cargo fmt --all` applied. Parent will run final workspace checks and SQLx prepare verification. No commits/push, no unrelated files touched.
