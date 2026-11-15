---
status: implemented
updated: 2026-11-15
---

# Domande aperte: temi

## Vincoli già chiariti

- [x] Il fork OpenMove può scegliere il tema iniziale senza bloccare la scelta dell'utente? Sì, con `SCHISACIACCOLE_DEFAULT_THEME` e cambio successivo nell'app. (2026-11-14)
- [x] Tema e modalità chiara/scura sono la stessa preferenza? No, sono indipendenti. (2026-11-14)
- [x] Quali erano i colori di marca forniti inizialmente? Blu `#4A45FB`, arancione `#F3704B`, rosa `#F493EA`, giallo `#F9E800`, verde `#B4F125`. Riferimenti del 2026-11-14, **superati il 2026-11-15** dai valori RGB dell'immagine.
- [x] Quale fonte numerica prevale? L'immagine sostituisce la palette iniziale: blu `#4A47F7`, arancione `#D86F4E`, rosa `#D38BE7`, giallo `#F1E600`, verde `#CBF02C`, bianco `#FFFFFF`. Per l'arancione vale RGB `216, 111, 78`, non il codice stampato incoerente. (2026-11-15)
- [x] Si implementa? La sospensione del 14 novembre è superata dall'autorizzazione esplicita del 15 novembre: implementazione richiesta e proposte residue confermate.
- [x] Quali ruoli OpenMove scuri? Blu per **Avvia / Avvia Timer** e altre azioni primarie; rosa per i tre titoli indicati nel design; arancione di marca nei turni; verde/giallo solo eventuali decorazioni. La vecchia proposta generica blu primario/rosa secondario è evoluta in questi ruoli. (2026-11-15)
- [x] Dove inserire i controlli? **Impostazioni** nel menu a tre puntini in alto a destra delle selezioni timer e meeting; inizialmente solo sezione **Tema** con **Predefinito della distribuzione / Schisaciaccole / OpenMove** e **Sistema / Chiaro / Scuro**. (2026-11-15)
- [x] Se il salvataggio fallisce? Scelta attiva nella sessione e piccolo testo rosso che segnala la mancata persistenza, senza modali. Posizione precisa e testo restano da validare. (2026-11-15)
- [x] Preferenza assente, non valida o tema rimosso? Ripiego sul predefinito della build; messaggio rosso informativo per dato non valido o tema rimosso, nessun errore per assenza normale. (2026-11-15)
- [x] ID, aggiornamenti e build non valida? ID stabili `schisaciaccole` e `openmove`; il predefinito segue la distribuzione, una scelta esplicita resta; variabile assente usa l'originale, vuota o sconosciuta fa fallire la build. (2026-11-15)
- [x] Quale perimetro iniziale? Catalogo incluso nell'app, nessun file esterno, nessun selettore libero. (2026-11-15)
- [x] Chi propone OpenMove chiaro? Proposta creativa delegata, da provare e validare nell'app; base concreta nel design, ancora preliminare. (2026-11-15)

## Da validare

- [x] Palette preliminari confermate come base: OpenMove scuro con fondo `#222228`; chiaro quasi bianco, blu primario e titoli prugna. Palette semantiche complete implementate con test dei contrasti principali. (2026-11-15)
- [ ] Completare la prova visiva interattiva dei quattro abbinamenti, delle variazioni native di Sistema e di tutti gli stati sui desktop supportati.
- [x] Confermiamo **Sistema** come modalità iniziale quando non esiste una preferenza? Sì. (2026-11-15)
- [x] Preferenze locali: quale formato e percorso usare, e con quale separazione tra installazioni o fork? TOML nel percorso fisso `~/.config/schisaciaccole/config.toml`, dove `~` indica la home dell’utente, anche per i fork. La coesistenza con l’app originale non è considerata realistica: nessun namespace per fork o identificativo di distribuzione richiesto. Il `config.rs` attuale non persiste preferenze visive. (2026-11-15)
- [x] Confermati messaggi vicino ai controlli nella sezione **Tema**, testi proposti e **Predefinito della distribuzione** nel ripiego; errori di lettura distinti, con messaggio esplicativo e ripiego sul predefinito. (2026-11-15)
