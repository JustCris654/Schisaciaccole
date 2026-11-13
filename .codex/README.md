# Schisaciaccole agent setup

`AGENTS.md` is the Codex entry point; `.agents/project.md` supplies the verified project map, commands and rule routing. `CLAUDE.md` remains the Claude entry point. Their shared workflow is plan, delegate, review/validate, then an atomic commit by the orchestrator. Push only on explicit developer request.

## Files and profiles

- `.agents/rules/`: server API, core, desktop, database, deployment, CI/release, Rust, docs and common rules. Read matching rules explicitly; frontmatter is applicability metadata for Codex. `.claude/rules/` has matching domain rules. The copies are independent and must be kept aligned when changed.
- `.codex/agents/*.toml`: eight domain profiles (`api-dev`, `core-dev`, `desktop-dev`, `deploy-dev`, `ci-release`, `db-postgres`, `docs-keeper`, `security-reviewer`) plus six generic model profiles. `.claude/agents/*.md` provides the matching domain roles and three Claude implementation profiles.
- Normal Codex routing follows `AGENTS.md`: `impl-luna` uses `gpt-6-luna`/low for mechanical work; `impl-sol-medium` and `impl-sol-high` use `gpt-6.1-sol` for judgment-requiring and complex/risky work. Domain profiles default to Sol medium; include their instructions in a Sol high brief for risky tasks. `impl-sol-low`, `impl-sol-xhigh`, `impl-astra` are retained for explicit developer overrides. Details: `../.agents/rules/model-routing.md`.
- `.agents/memory/MEMORY.md` is an initially minimal project note index. `.agents/agent-memory/README.md` describes optional role notes. These Markdown files do not establish automatic runtime memory. No imported user preferences are retained.
- Project-local `.claude/skills/` and `.agents/skills/` have been removed as requested. Worktrees are excluded from the copied setup.

Profile availability/loading depends on the client. If custom profile selection is unavailable, read the TOML and pass its instructions plus the required model/effort in the subagent brief. Report missing delegation/model support rather than silently changing the repository workflow. Profiles inherit session sandbox/approval settings: security-reviewer is behaviorally read-only; docs-keeper edits documentation only. The Claude reviewer has no Write/Edit tools and must also keep Bash commands read-only.

## Hooks and permissions

`.claude/settings.json` carries Claude permission patterns and two shell hooks. `guard-git.sh` requests confirmation for ordinary `git push` commands; it is not a comprehensive shell parser. Permission settings and explicit authorization remain required. `rust-fmt.sh` formats edited Rust files using the nearest explicit Cargo edition, walking through inherited workspace editions (2024 here). The hooks require Bash/jq and, for formatting, rustfmt.

`.codex/hooks.json` retains the existing graphify `PreToolUse` configuration with a PATH lookup and availability guard, avoiding a machine-specific executable path. It runs `graphify hook-check` only when installed. This file does not guarantee the active client supports or loads these hooks; verify client support. Claude permissions/hooks are not translated into native Codex permission controls.

Documentation follows the local glossaries and `.agents/rules/docs.md`. The team document link belongs to this project. Discover tools and verify access before external edits; if inaccessible, report pending proposed changes without claiming an update. No global runtime configuration is changed by this setup.
