# Google Groups independent review

Read-only review of feature code; owns only this review note. Preserve other agents' edits. No commits.

Reviewed repository/domain rules, implementation handoff, core merge/contracts, server traversal/routes/OAuth handling, desktop editor/list/Daily/reconnect changes, and mock coverage. Queried existing Graphify graph before source verification; graph predates feature changes.

Core and desktop: no blocking logic defect found. Direct attendee metadata wins case-insensitively, incomplete roots remain atomic, derived self uses authenticated identity, optional inheritance is coherent, fresh plan attendees drive editor equal reset and subsequent direct/Daily starts, warnings preserve Calendar omission separately, reconnect is explicit.

Server: request-local caches, HTTP/body limits, ancestry cycle checks, depth/pages/request/record/staged-leaf bounds, conservative lookup 404, atomic root publication, and both endpoint pipelines match the plan. Provider error strings are sanitized.

Findings sent to server agent and orchestrator, now resolved:

- Stored groups scope was checked before a possible second access-token refresh. It is now reread before expansion so a reduced grant exposes reconnect.
- An older in-flight refresh could overwrite reconnect credentials and repopulate stale cache. Both paths now share a transaction advisory lock.
- Initial user/credential DB reads occurred outside the five-second expansion timeout. These reads are now deadline bounded with fallback.

Follow-up source review: all three findings are addressed in the current implementation. Effective scopes are reread after token retrieval within the deadline. Initial identity/credential reads are deadline wrapped. Both login persistence/cache invalidation and refresh read/network/persistence/cache mutation use the same per-user PostgreSQL transaction advisory lock; all credential queries use its connection, avoiding pool reentrancy. No lock-order cycle or schema change was introduced. The existing per-process token cache still has its prior cross-instance limitations.

Added staged-leaf cache-reuse regression and opaque OAuth provider-error tests inspected. Also inspected delayed-refresh/reconsent regression, second uncached refresh with reduced scope, and no-new-refresh-token reconnect cases for omitted, added and reduced scope. These exercise the reported failures and grant compatibility behavior. No outstanding source-level review blocker remains.

Final disposition: approved. Server regressions passed, and the orchestrator reports all required workspace checks passing: format, offline workspace build, 804 tests with zero failures, and strict workspace clippy. SQLx prepare and final `--check` passed without metadata changes. The last initial-timeout fallback deduplication change uses the already-reviewed pure merge helper; no additional issue found. `git diff --check` passed during review.

No feature code changes or independent test run in this review pass; execution results come from the server agent/orchestrator. Glossaries remain removed per developer instruction. The prior access check found the external team document unavailable; no external update or live Google test is claimed. Feature commit is `9985484`; progress-documentation commit remains pending with the orchestrator. Concurrent timer ClockCell/ClockText edits were preserved outside the feature commit.
