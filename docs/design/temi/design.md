---
status: proposed
updated: 2026-11-15
---

# Temi selezionabili e predefinito della distribuzione

Il sistema di temi è implementato nel desktop. Il 15 novembre 2026 l'utente ha autorizzato l'implementazione e confermato le palette preliminari, il fondo scuro `#222228`, i messaggi vicino ai controlli e il ripiego per gli errori di lettura. Il [registro delle decisioni](./decisions.md) conserva la cronologia; [domande aperte](./open-questions.md) distingue le decisioni risolte dalle prove visive ancora necessarie.

## Obiettivo e vincoli confermati

L'app deve offrire un catalogo iniziale di temi curati. Chi distribuisce una build può scegliere il tema iniziale con `SCHISACIACCOLE_DEFAULT_THEME`; chi usa l'app può cambiarlo successivamente. Il fork OpenMove può quindi partire con il proprio tema senza vincolare la scelta dell'utente.

Il colore di marca non coincide con la modalità chiara o scura: tema e modalità sono due scelte indipendenti. Un tema deve funzionare in entrambe le modalità.

## Implementazione

Il catalogo degli ID in [`theme_catalog.rs`](../../../crates/app/src/theme_catalog.rs) è condiviso dal build script e dall'app. [`palettes.rs`](../../../crates/app/src/palettes.rs) definisce quattro palette semantiche complete; [`theme.rs`](../../../crates/app/src/theme.rs) risolve e salva le preferenze TOML. [`main.rs`](../../../crates/app/src/main.rs) aggiorna la finestra principale e le finestre delle scorciatoie aperte senza modificare lo stato dei meeting.

[`theme.slint`](../../../crates/app/ui/theme.slint) espone i token tramite `AppTheme`; un piccolo componente Slint non mostrato osserva `Palette.color-scheme` senza modificarne il binding nativo. Le finestre visibili ricevono questi aggiornamenti e applicano la modalità anche ai widget standard; Chiaro e Scuro sono override locali e Sistema ripristina il valore nativo corrente. I controlli di [`app-window.slint`](../../../crates/app/ui/app-window.slint) e [`editor-controls.slint`](../../../crates/app/ui/editor-controls.slint) usano token anche per focus, stati hover/premuto, selezione, avvisi, errori, turni e coriandoli. Nessun colore esadecimale resta scritto direttamente nei componenti Slint.

## Catalogo confermato

| ID stabile | Nome mostrato | Intento |
| --- | --- | --- |
| `schisaciaccole` | Schisaciaccole | Conservare l'identità visiva originale e il comportamento della build senza configurazione. |
| `openmove` | OpenMove | Usare i colori di marca OpenMove in una palette adatta all'app. |

Gli ID servono alla build e alla preferenza salvata: non dipendono dal nome mostrato né dalla lingua. Una rinomina visibile non deve cambiare l'ID. Entrambi i temi sono inclusi nel catalogo anche quando uno viene scelto come predefinito della distribuzione.

Ogni tema definisce una palette completa chiara e una scura con gli stessi ruoli semantici. Non basta sostituire il viola con un altro valore: superfici, testo, stati e accenti devono restare coerenti e leggibili insieme. Le palette complete sono in Rust e le combinazioni principali di testo e azione hanno test di contrasto; la verifica visiva su desktop resta necessaria.

### Colori di riferimento OpenMove confermati

La nuova immagine di riferimento sostituisce i valori scritti il 14 novembre. L'utente conferma i valori RGB dell'immagine come fonte autorevole, incluso l'arancione:

| Colore | Valore confermato |
| --- | --- |
| Blu | `#4A47F7` — `rgb(74, 71, 247)` |
| Arancione | `#D86F4E` — `rgb(216, 111, 78)` |
| Rosa | `#D38BE7` — `rgb(211, 139, 231)` |
| Giallo | `#F1E600` — `rgb(241, 230, 0)` |
| Verde | `#CBF02C` — `rgb(203, 240, 44)` |
| Bianco | `#FFFFFF` — `rgb(255, 255, 255)` |

L'esadecimale dell'arancione stampato nell'immagine (`#D26F50`) è incoerente con il suo RGB: vale `#D86F4E`. I precedenti blu `#4A45FB`, arancione `#F3704B`, rosa `#F493EA`, giallo `#F9E800` e verde `#B4F125` sono superati. Questi riferimenti di marca non costituiscono ancora una palette UI completa.

### OpenMove scuro: ruoli confermati e fondo confermato

- **Blu di marca** per le azioni primarie, tra cui **Avvia** e **Avvia Timer**, con testo e stati hover/premuto da verificare.
- **Rosa di marca** per i titoli **I meeting di oggi**, **Quando finisce il meeting** e **Partecipanti di questo meeting**.
- **Arancione di marca** all'interno dei turni di parola. Il suo uso non assegna automaticamente quel colore agli avvisi o al focus.
- **Verde e giallo** come eventuali accenti decorativi, per esempio nei coriandoli, senza significato automatico di successo o avviso.

Si mantiene il fondo pagina esistente `rgb(34, 34, 40)` (`#222228`), accettabile per l'utente anche senza una nuova alternativa. Pannelli, bordi, testi secondari e stati sono definiti dalla palette completa in Rust.

### OpenMove chiaro: base confermata

L'utente ha confermato la proposta cromatica chiara come base dell'implementazione. Le prove nell'app possono portare a successive rifiniture:

| Ruolo | Base confermata |
| --- | --- |
| Pagina / pannelli | Quasi bianco neutro `#FAFAFC` / bianco `#FFFFFF`. |
| Testo principale / secondario | `#25232C` / `#5D5865`. |
| Bordi | `#A39AAE` (rafforzato per i bordi funzionali). |
| Azioni primarie | Blu di marca, con testo e varianti da verificare nei controlli reali. |
| Titoli | Prugna scuro `#5C2D68`, per mantenere il richiamo al rosa con contrasto sul chiaro. |
| Superfici rosa / testo | Rosa tenue `#F4E8F7` con testo scuro `#45274E`. |
| Turni | Arancione di marca in accenti o superfici attenuate, con testo scuro e stati da definire. |

Non usare il rosa chiaro di marca come testo sul bianco senza una variante adatta. Tutte le combinazioni, incluse icone e focus, richiedono verifica del contrasto e prova nei controlli reali. La palette Schisaciaccole deve conservare l'identità originale in entrambe le modalità; eventuali adattamenti restano da validare.

### Ruoli dei colori

Le palette condividono i token semantici di `AppTheme`, completi per ciascuna modalità:

| Famiglia | Ruoli da coprire |
| --- | --- |
| Superfici | Pagina, pannelli e schede, bordi e separatori. |
| Testo | Principale, secondario, collegamenti e titoli del timer; testi sui diversi fondi colorati. |
| Marca e azioni | Primario e secondario, relativi testi e fondi attenuati, stati hover e premuto. |
| Interazione | Focus da tastiera, selezione, controlli disabilitati. |
| Stati semantici | Avviso, errore o azione distruttiva, successo dove presente; testo, fondo e bordo necessari. |
| Timer e turni | Timer normale, pausa, soglia di avviso, scadenza, turno attivo e turni in attesa. |
| Decorazioni | Accenti e coriandoli, senza significato di stato. |

Ruoli diversi possono condividere un valore all'interno di un tema, ma devono poter variare separatamente. Le trasformazioni dei colori usate dai controlli vanno verificate negli stati reali; se non garantiscono leggibilità, il tema deve fornire valori espliciti. Il lavoro riguarda i colori: geometrie, tipografia e animazioni non diventano opzioni del tema.

## Scelta dell'utente e modalità

Il menu a tre puntini in alto a destra nelle schermate di **selezione del timer** e **selezione del meeting** contiene la nuova voce **Impostazioni**. Le impostazioni inizialmente contengono solo la sezione **Tema**, con due controlli indipendenti:

| Controllo | Opzioni confermate |
| --- | --- |
| Tema | **Predefinito della distribuzione**, **Schisaciaccole**, **OpenMove**. |
| Modalità | **Sistema**, **Chiaro**, **Scuro**. |

La scelta si applica durante l'uso e viene salvata localmente, senza account e senza dipendenza dal backend. Login e logout non devono cambiare una preferenza visiva locale.

**Predefinito della distribuzione** è una scelta esplicita: significa seguire il valore incorporato nella build, anche dopo un aggiornamento. Non va salvato come copia dell'ID corrente, altrimenti un aggiornamento della distribuzione non potrebbe cambiarlo. Scegliere esplicitamente un tema, invece, deve conservarlo anche se cambia il predefinito della build.

Con **Sistema**, una variazione del sistema cambia la variante chiara/scura senza cambiare il tema. Ripristinare il tema della distribuzione non deve alterare la modalità. In assenza di una preferenza, la modalità iniziale confermata è **Sistema**.

### Risoluzione e messaggi

1. Un ID salvato e presente nel catalogo prevale sul predefinito della build.
2. La scelta **Predefinito della distribuzione**, oppure l'assenza della preferenza, usa il tema incorporato nella build.
3. Un tema rimosso dal catalogo o una preferenza non valida ripiega sul predefinito valido della build, senza impedire l'avvio. Si mostra **Predefinito della distribuzione** nel selettore; il messaggio spiega il ripiego.

Gli errori sono piccoli testi rossi nell'interfaccia, senza modali. Sono collocati nella sezione **Tema**, vicino ai controlli interessati:

- Salvataggio fallito: «Scelta applicata per questa sessione, ma non salvata.» La scelta applicata resta attiva nella sessione; non si promette che sopravviva al riavvio.
- Dato non valido o tema rimosso: «Tema salvato non disponibile. Uso il predefinito della distribuzione.»

Una preferenza normalmente assente non è un errore e non mostra un avviso. Un errore di lettura ripiega sul predefinito e mostra «Impossibile leggere le preferenze. Uso il predefinito della distribuzione.» Una modalità non valida usa Sistema con il messaggio «Modalità salvata non valida. Uso la modalità Sistema.» Un tema non valido conserva una modalità valida, e viceversa. L'utente ha confermato posizione, testi proposti e ripiego per gli errori di lettura.

Il TOML usa le chiavi `theme = "distribution" | "schisaciaccole" | "openmove"` e `mode = "system" | "light" | "dark"`. Una scrittura sostituisce il file atomicamente tramite un file temporaneo nella stessa directory e conserva eventuali chiavi TOML estranee. Un errore lascia attiva la scelta della sessione e conserva il file precedente. Un file TOML non valido può essere riparato selezionando una scelta valida nelle impostazioni.

Le preferenze locali sono salvate in formato **TOML** nel file **`~/.config/schisaciaccole/config.toml`**, dove `~` indica la home dell’utente. Il percorso è fisso e vale anche per i fork: la coesistenza dell’app originale e di un fork non è considerata uno scenario realistico, quindi non sono richiesti namespace per fork o identificativi di distribuzione. Non serve introdurre un servizio di configurazione o sincronizzare la scelta nelle preferenze dei meeting.

## Predefinito a compilazione

Comportamento confermato per `SCHISACIACCOLE_DEFAULT_THEME`:

| Valore nella build | Risultato |
| --- | --- |
| Variabile assente | `schisaciaccole`, il tema originale. |
| `schisaciaccole` o `openmove` | Tema corrispondente come predefinito. |
| Variabile presente ma vuota, oppure ID sconosciuto | Fallimento della build con messaggio descrittivo e lista degli ID ammessi. |

La variabile appartiene alla compilazione del desktop, come il backend URL; non è una configurazione del server né una lettura dell'ambiente a ogni avvio. La validazione deve usare lo stesso catalogo degli ID disponibili. Il build script dichiara `cargo:rerun-if-env-changed=SCHISACIACCOLE_DEFAULT_THEME` affinché Cargo riconsideri la scelta quando cambia.

Esempio della build per il fork OpenMove, con URL segnaposto:

```sh
SCHISACIACCOLE_BACKEND_URL=https://schisaciaccole.example.com \
SCHISACIACCOLE_DEFAULT_THEME=openmove \
cargo run --release -p schisaciaccole
```

La scelta del tema deve funzionare anche senza `SCHISACIACCOLE_BACKEND_URL`. La configurazione operativa è documentata anche nel README.

## Architettura

Il catalogo dei temi e la risoluzione della preferenza sono implementati in Rust dentro `crates/app`. Il build script valida il predefinito; la gestione locale delle preferenze conserva la scelta dell'utente. Non occorrono modifiche a `crates/core`, API, server o database.

`AppTheme` resta il punto di accesso dei componenti Slint ai colori. Rust comunica il tema scelto; Slint usa i binding per aggiornare i colori senza duplicare la logica nei singoli componenti. La separazione concreta dei file va mantenuta semplice e coerente con i moduli esistenti. I colori scritti direttamente nella UI vanno esaminati per ruolo e collegati ai token appropriati, incluse le decorazioni.

Non serve un motore di temi estendibile nella prima versione: bastano catalogo curato, scelta locale e predefinito di build. File di tema esterni e selettore libero di colori restano fuori dallo scopo iniziale. Non si introduce `THEME_FILE`.

## Verifiche

- Build senza variabile, con ciascun ID valido, con valore vuoto e con ID sconosciuto; cambio della variabile senza pulizia manuale della build.
- Avvio iniziale, scelta esplicita, riavvio, ritorno a **Predefinito della distribuzione** e aggiornamento che modifica il predefinito. Una scelta esplicita deve restare invariata; chi segue la distribuzione deve ricevere il nuovo predefinito.
- Preferenza assente, non valida o riferita a un tema rimosso; errore di scrittura con scelta attiva nella sessione e messaggio inline; errore di lettura con ripiego e messaggio distinto confermati.
- Tutti i temi nelle modalità chiara, scura e di sistema, comprese le variazioni del sistema mentre l'app è aperta.
- Contrasto di testo, icone, bordi funzionali e focus da tastiera su tutti i fondi pertinenti; stati normale, hover, premuto, selezionato e disabilitato.
- Timer normale, pausa, avviso, scadenza, turni e Daily; messaggi di errore, dialoghi, schermata compatta e celebrazioni. Il significato degli stati deve restare riconoscibile senza affidarsi al solo colore.
- Cambio del tema senza modificare timer, partecipanti, navigazione o modalità; funzionamento locale anche senza backend e attraverso login/logout.

I test automatici coprono catalogo e validazione del predefinito, risoluzione e persistenza, errori di lettura/scrittura e contrasti principali. Un test con rendering software verifica il fondo effettivo e gli aggiornamenti nativi simulati, anche dopo un override e il ritorno a Sistema, conservando tema e pagina. Le prove interattive elencate sopra restano criteri di verifica visiva sui desktop supportati; non vanno confuse con la sola compilazione o con i test unitari.
