---
status: implemented
updated: 2026-11-15
---

# Decisioni: temi

L'utente ha autorizzato l'implementazione e confermato le proposte residue il 15 novembre 2026. Le righe storiche **Proposta** sono conservate per la cronologia e sono risolte dalle conferme più recenti.

| Data | Stato | Decisione o proposta |
| --- | --- | --- |
| 2026-11-15 | Confermato e implementato | Autorizzata l'implementazione del design. Confermati OpenMove chiaro come proposto, fondo scuro `#222228`, messaggi rossi vicino ai controlli, selettore su **Predefinito della distribuzione** nel ripiego ed errori di lettura distinti con ripiego sul predefinito. |
| 2026-11-15 | Implementato | Catalogo Rust condiviso dalla build, quattro palette semantiche, impostazioni locali live, TOML con sostituzione atomica e scelta della sessione conservata in caso di errore di scrittura; test di risoluzione, persistenza, catalogo e contrasto. |
| 2026-11-15 | Confermato | Le preferenze locali saranno salvate in TOML in `~/.config/schisaciaccole/config.toml`, dove `~` indica la home dell’utente. Il percorso fisso vale anche per i fork: la coesistenza con l’app originale non è considerata realistica e non richiede namespace per fork o identificativi di distribuzione. |
| 2026-11-15 | Confermato | Modalità iniziale **Sistema** in assenza di una preferenza salvata. |
| 2026-11-15 | Confermato | I valori RGB della nuova immagine sostituiscono la palette scritta del 14 novembre: blu `#4A47F7`, arancione `#D86F4E`, rosa `#D38BE7`, giallo `#F1E600`, verde `#CBF02C`, bianco `#FFFFFF`. Per l'arancione vale RGB `216, 111, 78`, non l'esadecimale stampato `#D26F50`. |
| 2026-11-15 | Confermato | OpenMove scuro: blu nelle azioni primarie (**Avvia**, **Avvia Timer**), rosa nei titoli **I meeting di oggi**, **Quando finisce il meeting**, **Partecipanti di questo meeting**, arancione di marca nei turni di parola. Verde e giallo sono eventuali decorazioni. |
| 2026-11-15 | Proposta | Mantenere il fondo scuro esistente `#222228`, accettabile per l'utente; completare superfici, stati e contrasti prima della validazione visiva. |
| 2026-11-15 | Confermato | La palette OpenMove chiara è delegata a una proposta creativa, da provare e validare successivamente nell'app. |
| 2026-11-15 | Proposta | OpenMove chiaro: pagina `#FAFAFC`, pannelli bianchi, azioni blu, titoli prugna `#5C2D68`, superfici rosa tenue `#F4E8F7` con testo scuro; niente rosa chiaro come testo sul bianco senza verifica. |
| 2026-11-15 | Confermato | Nel menu a tre puntini in alto a destra delle selezioni timer e meeting si aggiunge **Impostazioni**, inizialmente con la sola sezione **Tema** e i controlli tema (**Predefinito della distribuzione / Schisaciaccole / OpenMove**) e modalità (**Sistema / Chiaro / Scuro**). |
| 2026-11-15 | Proposta | Posizione dei piccoli messaggi rossi vicino ai controlli della sezione **Tema** e testi precisi da validare. |
| 2026-11-15 | Confermato | Errori con piccoli testi rossi inline, senza modali. Se il salvataggio fallisce, conservare la scelta applicata nella sessione e spiegare che non è stata salvata. |
| 2026-11-15 | Confermato | Preferenza assente, non valida o riferita a un tema rimosso: ripiego sul predefinito della build. Dato non valido o tema rimosso: breve informazione rossa; assenza normale: nessun errore. |
| 2026-11-15 | Proposta | Nel ripiego mostrare **Predefinito della distribuzione**; distinguere errore di lettura da preferenza normalmente assente, con messaggio esplicativo. Testi precisi da validare. |
| 2026-11-15 | Confermato | ID stabili `schisaciaccole` e `openmove`; **Predefinito della distribuzione** segue la build agli aggiornamenti, una scelta esplicita resta invariata. |
| 2026-11-15 | Confermato | Build senza variabile: tema originale. Variabile `SCHISACIACCOLE_DEFAULT_THEME` vuota o sconosciuta: errore di compilazione. |
| 2026-11-15 | Confermato | Catalogo curato incluso nell'app; esclusi file esterni, selettore libero e `THEME_FILE`. |
| 2026-11-15 | Risolta il 2026-11-15 | In precedenza formato, percorso e separazione delle preferenze tra fork erano aperti; la conferma successiva sceglie TOML, `~/.config/schisaciaccole/config.toml` e lo stesso percorso anche per i fork. |
| 2026-11-14 | Superato il 2026-11-15 | Il verde OpenMove `#B4F125` si aggiunge ai riferimenti di marca. |
| 2026-11-14 | Superato il 2026-11-15 | I riferimenti OpenMove sono blu `#4A45FB`, arancione `#F3704B`, rosa `#F493EA` e giallo `#F9E800`; l'utente apprezza particolarmente il rosa. Non sono ancora una palette UI completa. |
| 2026-11-14 | Confermato | Si documenta il design per una validazione congiunta prima dell'implementazione. |
| 2026-11-14 | Confermato | L'app avrà un catalogo iniziale curato di temi; la scelta del predefinito non impedirà all'utente di cambiarlo. |
| 2026-11-14 | Confermato | `SCHISACIACCOLE_DEFAULT_THEME` sceglierà il tema iniziale della build. Il fork OpenMove potrà scegliere il proprio tema iniziale. |
| 2026-11-14 | Confermato | Tema e modalità chiara/scura sono indipendenti. |
| 2026-11-14 | Confermata il 2026-11-15 | Catalogo iniziale con ID stabili `schisaciaccole` e `openmove`, ciascuno con palette completa chiara e scura. |
| 2026-11-14 | Evoluta il 2026-11-15 | Blu primario e rosa secondario per OpenMove; palette complete, varianti e contrasti da validare. Giallo e verde sono possibili accenti decorativi, senza assegnazione automatica ad avviso o successo. |
| 2026-11-14 | Proposta | Separare accenti di marca, turni, focus e stati semantici nei token, evitando una sostituzione globale dell'arancione attuale. |
| 2026-11-14 | Confermata il 2026-11-15 | Una scelta locale esplicita prevale sul predefinito di build; **Predefinito della distribuzione** segue la build anche negli aggiornamenti. Scelte mancanti o non valide ripiegano sul predefinito valido. |
| 2026-11-14 | Evoluta il 2026-11-15 | Variabile di build assente: tema originale. Variabile vuota o ID sconosciuto: errore descrittivo di compilazione. Dichiarare `cargo:rerun-if-env-changed=SCHISACIACCOLE_DEFAULT_THEME`. |
| 2026-11-14 | Proposta | Catalogo, preferenze locali e validazione nel desktop; binding tramite `AppTheme`. Nessuna dipendenza dal backend e nessuna modifica a core o server. |
| 2026-11-14 | Confermata il 2026-11-15 | File di tema esterni e selettore libero di colori esclusi dalla prima versione; nessuna variabile `THEME_FILE`. |
