---
paths:
  - "crates/core/**"
---

# Shared core

`schisaciaccole-core` contains pure time/quarter/countdown, attendee, speaking-turn, Daily and participant logic, editor rows and meeting preference rules (`plan.rs`, no weights or slot plans), plus shared API types. Pass time into domain functions; do not introduce wall-clock reads or UI/network/database dependencies.

Read the affected module and its tests. Preserve boundary behavior for elapsed meetings, participant counts, time allocation, pause/skip and midnight transitions where applicable. Keep API contracts aligned with both consumers. Test documented edge cases and update local glossaries for all changed item lines.
