# Codex model routing

`AGENTS.md` governs normal delegation. State model and effort choices in the plan and pass them explicitly to `spawn_agent`:

| Task | Model | Effort | Generic profile |
| --- | --- | --- | --- |
| Precise, mechanical, low-risk single-file work | `gpt-6-luna` | `low` | `impl-luna` |
| Rust, multi-file or judgment-requiring work; default when unsure | `gpt-6.1-sol` | `medium` | `impl-sol-medium` |
| Complex or risky architecture, async, CI or deployment work | `gpt-6.1-sol` | `high` | `impl-sol-high` |

Domain profiles default to Sol medium. For a risky domain task, use Sol high with the domain instructions in the brief. An explicit developer override takes precedence.

The retained `impl-sol-low`, `impl-sol-xhigh` and `impl-astra` profiles are available only for explicit developer overrides of the normal routing. Preserve their configured model/effort; Astra has no fixed effort. These profiles do not change the main session model.

When the client supports custom profiles, inspect their pinned settings before selecting one. If it cannot select profiles, read the corresponding TOML and pass its instructions with explicit model/effort to the available subagent tool. If delegation or the chosen model is unavailable, report the limitation; do not silently implement in the orchestrator or substitute a model.

All profiles inherit session permissions. Read-only and document-only roles describe behavioral scope, not a separate Codex sandbox.
