---
status: accepted
updated: 2026-11-14
---

# Piano di implementazione: preferenze dei meeting

Ogni passo è un commit atomico: dopo ognuno passano `cargo fmt --all --check`, `cargo build --workspace`, i test con `DATABASE_URL` e `cargo clippy --workspace --all-targets -- -D warnings`. I messaggi di commit descrivono la modifica, senza riferimenti a questo piano.

Tra il passo 3 e il passo 4 il repository compila e i test passano, ma un'app costruita a quel commit chiama rotte che il server non ha più. Non è un problema perché app e server si rilasciano insieme dall'ultimo commit.

| # | Commit | Contenuto | Agente e modello |
| --- | --- | --- | --- |
| 1 | `refactor(core): remove the unused weighted slot schedule` | Toglie `allocate_ms`, `SlotSchedule`, `SlotView`, `MIN_SLOT_MS` e i loro test da `slots.rs` e dagli export di `lib.rs`. | `core-dev`, `haiku`: rimozione meccanica in un file più un export. |
| 2 | `feat(core): add meeting preference types and editor rules` | Aggiunge `PersonKind`, `PreferencePerson`, `PutPreferenceRequest`, `MeetingPreference`, `PREFERENCES_PATH`, `preference_path`, il campo `Meeting::preference` (accanto a `has_template` per ora) e le funzioni `editor_rows`, `merge_rows` e `preference_people` con i test. Aggiorna i letterali `Meeting` in app e server. | `core-dev`, `sonnet`: regole con casi limite (rinomine, email duplicate, rinomine conservate, unione della bozza). |
| 3 | `feat(server)!: replace slot templates with meeting preferences` | Migrazione `0003`, `preferences.rs`, store, `meetings.rs` che riempie `preference`, rotte in `app.rs`, rimozione di `meeting_plan.rs` e `templates.rs`, test d'integrazione, `.sqlx/`. | `api-dev` con `sonnet`: più file, SQLx, contratto HTTP; nessuna concorrenza nuova. |
| 4 | `feat(app): open the editor locally and save meeting preferences` | Bozze per evento, editor senza caricamento, Daily da editor, nomi di avvio comuni a lista ed editor, casella sulle righe aggiunte, client e worker delle preferenze, rimozione del worker del piano, Slint. | `desktop-dev` con `sonnet`: `state.rs` e Slint ampi ma senza async nuovo; se la revisione trova problemi nella gestione dei risultati in volo dei worker si passa a `opus`. |
| 5 | `refactor(core): remove slot plans and weighted templates` | Toglie `has_template`, `EntryKind`, `TemplateEntry`, `PutTemplateRequest`, `Template`, `PlanSource`, `PlanEntry`, `SlotPlan`, le costanti e funzioni dei percorsi vecchi, `build_plan`, `plan_to_editor`, `editor_to_template`, `FREE_TIME_LABEL`, `PARTICIPANT_WEIGHT_SECS`; aggiorna i letterali. | `core-dev`, `haiku`: rimozione meccanica guidata dal compilatore. |
| 6 | `docs: describe meeting preferences and the local editor` | Guida (Markdown e HTML), panoramica, `README.md`, team doc e glossari se accessibili. | `docs-keeper`, `sonnet`: più documenti e diagrammi da tenere coerenti. |
| 7 | `docs(rules): update agent rules for meeting preferences` | `.claude/rules/{api,desktop,core}.md` e copie in `.agents/rules/`. | `impl-haiku`, `haiku`: testo con specifica precisa. |

Validazione dell'orchestratore dopo ogni passo: diff contro questo design e le regole del repository, poi i controlli sopra. Per il passo 3 anche una revisione `security-reviewer` delle query (filtro `user_id`) e della validazione.
