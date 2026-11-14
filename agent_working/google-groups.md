# Google Groups participant expansion

Status: Google Groups expansion, OAuth handling, desktop warnings/reconnect and documentation are complete and reviewed. Feature commit: `9985484`. All four required workspace checks pass, including 804 tests with no failures. SQLx prepare and `--check` pass with unchanged metadata. Progress-documentation commit remains pending with the orchestrator; no push.

Ownership: core, server and desktop agents completed their scoped implementation; the independent reviewer finalized this progress file. Preserve all edits by others.

Pre-existing state: `SECURITY-AUDIT.md` is untracked; no tracked modifications at this subagent's analysis start. The orchestrator observed prior in-flight edits subsequently committed by another agent as `facc213 feat(app): restore optional meeting turn editor`; preserve those edits and that commit. Existing meeting editor progress lives in `meeting_editor.md` and `meeting_editor_docs.md`. Repository instructions still reference `README.org`, but the current source is `README.md`; local glossaries are absent (previous task notes say intentionally removed).

Implemented areas: core additive participant contracts and pure merge; server Cloud Identity resolver, common list/plan pipeline, OAuth grant/cache coordination, configuration and mock coverage; desktop fresh participants, notices and explicit reconnect; `.env.example` and `README.md`.

Confirmed read paths: `list_meetings` maps Calendar lists, while `get_meeting_plan` separately re-reads and maps a Calendar event. Both must use the same expansion pipeline before participant logic. The editor resets equal slots from its original Meeting, so plan-only expansion would become inconsistent.

OAuth: login requests Cloud Identity read-only groups access. Explicit grants are persisted, omitted grants with reused credentials remain unchanged, successful reconnect invalidates cached access, and refresh rechecks reduced grants. Per-user PostgreSQL transaction advisory locks serialize credential refresh with reconnect persistence/cache changes.

Developer decisions: retain and visibly flag invited addresses even on ambiguous lookup `404`; keep glossaries removed. No glossary recreation is authorized.

## Implemented core contract

- `api::ParticipantResolution { incomplete: bool, reauthorization_required: bool }`, default false and serde-defaulted fields, carried as `participant_resolution` on both `Meeting` and `SlotPlan`.
- `SlotPlan` additionally carries serde-defaulted `attendees: Option<Vec<Attendee>>` and `attendees_omitted: bool`. Missing attendees means an older backend; `Some([])` is a fresh empty resolved list. The desktop can use the newly read participants for equal reset and fresh warning state.
- `attendee::GroupMember { email, name: Option<String> }`, `GroupExpansion { email, outcome }`, and `GroupExpansionOutcome::Complete(Vec<GroupMember>) | Incomplete { reauthorization_required: bool }` are plain pure inputs.
- `merge_group_attendees(&[Attendee], &[GroupExpansion], authenticated_email: &str) -> ResolvedAttendees { attendees, resolution }` merges by ASCII-insensitive email. Direct survivors always keep every original field. Incomplete roots survive atomically. Complete roots disappear, including empty groups. Members use NeedsAction and authenticated identity, inherit root optional, and become required if any derived root is required. First direct duplicate and first member name/email win; unmatched outcomes have no effect.
- Public types and helper are available under their modules and re-exported at the core crate root.

Validation: core has 227 passing tests, including 17 new merge/compatibility cases. Final workspace format, build, tests and strict clippy all pass; see final handoff below.

## Completed implementation plan

1. Pure core contracts and merge. Add additive defaulted participant-resolution metadata to Meeting and SlotPlan so successful fallback is visible, including the independently re-read editor plan. Keep Calendar's attendees_omitted meaning intact. Add a pure merge operation receiving direct attendees and completed expansion results: normalize email for matching, insert all direct people first so their whole Attendee wins regardless of ordering, then add deduplicated group members with NeedsAction and is_self computed from the authenticated email. Reuse existing email-to-name helper when Cloud Identity supplies no display name. Keep response, optional, name and self unchanged for direct attendees. Define inherited optional behavior for derived members explicitly in the implementation brief (proposed: inherit the top-level invitation's optional flag). No I/O in core.

2. Server Cloud Identity client and expansion coordinator. Call only GET groups:lookup and GET groups/{id}/memberships, follow page tokens, detect repeated tokens, validate group resource paths before constructing URLs, and sanitize errors to omit URLs, response bodies and personal data. Resolve nested groups iteratively with a visited/path set and memoization scoped to one authenticated request; never share membership cache across users. Acyclic repeated subgroups reuse completed results; back edges do not recurse. Use one budget across the whole meetings request, not a new one per attendee or event.

   Proposed bounded defaults: total expansion deadline 5 seconds, per-request network timeout capped at remaining time (maximum 2 seconds), maximum nesting depth 8, at most 64 Cloud Identity HTTP requests including lookup and membership pages, at most 10 membership pages per group, and at most 5,000 discovered membership records per request. Check budgets before every request and before queuing more work; use tokio timeout around the full expansion phase so slow body reads are bounded too. Existing router timeout is 30 seconds, so expansion must not multiply Calendar's existing latency by attendee count.

   Atomic fallback per top-level group: stage its full leaf set privately and publish it only when traversal completes within all bounds. On inaccessible/disabled API, 403/429/5xx, timeout, malformed JSON, invalid member, pagination/depth/request/member bound or incomplete nested group, discard that root's staged members, retain its original attendee exactly once and mark resolution incomplete. Preserve successful independent roots and every direct attendee. This prevents simultaneously counting a group placeholder and some of its children. A cycle edge ends recursion without emitting a group as a person; expose incomplete rather than claim a complete list if a cyclic component cannot be fully resolved. Empty successfully resolved groups contribute zero people. Lookup 404 preserves the original invitation with an incomplete warning, per developer choice.

3. Apply one pipeline to both list_meetings and get_meeting_plan before participant/plan construction. Fetch authenticated user email via existing store::find_user, not Calendar group self flags. Keep all Calendar operations read-only. Keep access token refresh bounded to one retry where needed and account for Cloud Identity requests in the global expansion budget. Cloud Identity failures must yield successful Calendar data plus resolution metadata, not erase meetings or send the desktop into forced login.

4. OAuth and configuration. Preserve GOOGLE_CALENDAR_SCOPE and add the Cloud Identity read-only groups scope to authorization. Add an overridable Cloud Identity base URL for wiremock. Missing new scope must produce incomplete resolution and an explicit reconnect opportunity while Calendar keeps working; ordinary permission/organization API failures must not be mistaken for missing consent. Invalidate cached Google access token after a successful credential update. Test login with a new refresh token, no refresh token and old credentials, explicitly missing granted scope, omitted scope field, denied consent, and retry behavior. Do not persistently infer new scope from a new requested scope when reusing old credentials.

5. Desktop. Show a readable incomplete participant notice in meeting rows and the editor. Use freshly read SlotPlan resolution metadata alongside the original Meeting metadata when opening the editor, so a changed group failure is not hidden. Avoid adding derived NeedsAction members under existing copy that says accepted participants: update count copy to neutral participants. Keep direct Avvia, Daily and equal-reset behavior on the same expanded list. Reauthorization should be a user action and cannot auto-repeat on periodic refresh. Never block start or editor editing because expansion failed.

6. Documentation and validation. Update README.md and .env.example with Cloud Identity API activation, read-only scope, per-user permissions, best-effort atomic fallback and bounded resolution. Keep glossaries removed per developer decision. Verify actual access before claiming a linked external team document update. Run format, build, workspace tests and strict workspace clippy before orchestrator commits; run mock-driven core/server/app checks during implementation.

## Implementation ownership and commit plan

- Core/contracts: gpt-6.1-sol, high reasoning, because precedence, additive wire compatibility and warning propagation affect two consumers. Own core files and core tests.
- Server: gpt-6.1-sol, high reasoning, because bounded async traversal, OAuth grant handling and both endpoints are coupled. Own server files, mock integration tests and test helpers. Coordinate exact shared types with core before editing. Commit complete server expansion and OAuth support together after required checks.
- Desktop/documentation: gpt-6.1-sol, medium reasoning for Rust/Slint orchestration and warning/reconnect behavior. Own app files, README.md and .env.example after the server interface is stable. Commit warning/reconnect UI and directly associated usage documentation together.
- Orchestrator reviews each scoped diff, preserves other workers' changes and performs allowed full checks. The coherent feature needs one atomic commit across core/server/UI because added fields require literal updates in all consumers; do not commit intermediate states that cannot compile. Push only when requested.

## Required test cases

Core: full direct metadata precedence in both orders, case-insensitive duplicate direct/derived/multiple groups, NeedsAction and identity self for members, name fallback, empty expansion, direct declined attendees preserved, derived optional policy.

Server mocks: paginated memberships, nested groups, shared subgroup memoization, cycles, repeated page tokens, empty groups, permission denial, disabled API, missing scope, 404 choice, 429/5xx, malformed payloads/members, depth/request/member/page/time limits, atomic root rollback after a successful first page, independent successful roots preserved, both meetings and plan routes, user-isolated requests and fresh membership state on next request, no Calendar writes, no refresh loop after reconsent/no refresh token/partial grant.

Desktop: warning visible in meeting metadata and editor for Calendar omission or group resolution failure, plan refresh failure reflected despite original Meeting success, neutral participant count wording, explicit reconnect without repeated automatic login, start/Daily/reset still available with fallback.

## Final handoff — 2026-11-14

Implementation and independent source review are complete. The earlier interrupted handoff's compilation issues and review findings are resolved: cached subgroup traversal consumes the staged-leaf budget, effective scopes are reread after a second access-token refresh, OAuth provider errors are sanitized, initial identity/grant reads share the expansion deadline, and refresh/reconnect credential races are serialized. Regression tests cover these behaviors.

Final orchestrator validation passes: `cargo fmt --all --check`, `SQLX_OFFLINE=true cargo build --workspace`, workspace tests against temporary PostgreSQL, and `cargo clippy --workspace --all-targets -- -D warnings`. All 804 tests pass: app 249, core 227, server library 257, auth 31, groups routes 5, meetings 26 and templates 9. SQLx prepare and final `--check` passed with unchanged offline metadata.

Each authenticated expansion request is bounded by five seconds, 64 HTTP requests, eight nesting levels, ten pages per group, 5,000 discovered records and 5,000 cumulative staged leaves (including cached traversal), two seconds per HTTP call and 2,000,000 bytes per response body. All fallback remains atomic per root. Both Calendar routes use the same resolver; editor reset, direct start and Daily use the resolved participants.

Developer decisions remain: lookup 404 preserves and flags the invited address; glossaries stay removed and were not recreated. The prior access check found the linked external team document unavailable; no external update is claimed. Proposed external updates are the implemented participant resolution, explicit reconnect and bounded fallback behavior documented in README. README includes manual `team-backend@openmove.com` checks, but no live Google verification was performed.

Feature commit `9985484` was created by the orchestrator after validation. The progress-documentation commit remains pending; no push. Unrelated AGENTS.md changes, untracked SECURITY-AUDIT.md and concurrent timer ClockCell/ClockText edits were preserved and excluded from the feature commit. Temporary test PostgreSQL at localhost:55439, data `/tmp/google-groups-pg`, was stopped after verification; no production database was used.
