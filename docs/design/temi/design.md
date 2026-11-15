---
status: accepted
updated: 2026-11-15
---

# Temi selezionabili e predefinito della distribuzione

Il sistema di temi è implementato nel desktop. Il catalogo è guidato dai dati e contiene quattro temi: Schisaciaccole e tre temi OpenMove (Bubblegum, Lavanda elettrica, Campo Blu), ciascuno con una palette chiara e una scura. Il [registro delle decisioni](./decisions.md) conserva la cronologia; [domande aperte](./open-questions.md) distingue le decisioni risolte dalle prove visive ancora necessarie. Le palette esplorate sono nella [pagina di prova](./mockup-openmove.html), fonte visiva dei sei abbinamenti.

## Obiettivo e vincoli confermati

L'app offre un catalogo di temi curati. Chi distribuisce una build può scegliere il tema iniziale con `SCHISACIACCOLE_DEFAULT_THEME`; chi usa l'app può cambiarlo successivamente. Il fork OpenMove può quindi partire con uno dei propri temi senza vincolare la scelta dell'utente.

Il colore di marca non coincide con la modalità chiara o scura: tema e modalità sono due scelte indipendenti. Un tema deve funzionare in entrambe le modalità.

## Implementazione

Il catalogo ordinato `THEME_CATALOG` (variante di `ThemeId`, ID stabile, nome) in [`theme_catalog.rs`](../../../crates/app/src/theme_catalog.rs) è condiviso dal build script e dall'app. [`palettes.rs`](../../../crates/app/src/palettes.rs) definisce una palette chiara e una scura per ogni tema (otto in totale), con una `match` esaustiva sul tema; [`theme.rs`](../../../crates/app/src/theme.rs) risolve e salva le preferenze TOML. [`main.rs`](../../../crates/app/src/main.rs) aggiorna la finestra principale e le finestre delle scorciatoie aperte senza modificare lo stato dei meeting.

[`theme.slint`](../../../crates/app/ui/theme.slint) espone i token tramite `AppTheme`; un piccolo componente Slint non mostrato osserva `Palette.color-scheme` senza modificarne il binding nativo. Le finestre visibili ricevono questi aggiornamenti e applicano la modalità anche ai widget standard; Chiaro e Scuro sono override locali e Sistema ripristina il valore nativo corrente. I controlli di [`app-window.slint`](../../../crates/app/ui/app-window.slint) e [`editor-controls.slint`](../../../crates/app/ui/editor-controls.slint) usano token anche per focus, stati hover/premuto, selezione, avvisi, errori, turni e coriandoli. Nessun colore esadecimale resta scritto direttamente nei componenti Slint.

## Catalogo

| ID stabile | Nome mostrato | Intento |
| --- | --- | --- |
| `schisaciaccole` | Schisaciaccole | Conservare l'identità visiva originale e il comportamento della build senza configurazione. |
| `openmove-1` | Bubblegum | OpenMove con rosa di marca: chiaro bianco e magenta, scuro grafite e blu. |
| `openmove-2` | Lavanda elettrica | OpenMove sobrio e deciso: chiaro bianco e blu, scuro nero e lime. |
| `openmove-3` | Campo Blu | OpenMove a tinta unita: chiaro lavanda, scuro con la pagina blu di marca. |

Gli ID servono alla build e alla preferenza salvata: non dipendono dal nome mostrato né dalla lingua. Una rinomina visibile non deve cambiare l'ID. Il vecchio ID `openmove` è rimosso: un valore salvato ripiega sul predefinito della build con l'avviso inline, e come `SCHISACIACCOLE_DEFAULT_THEME` fa fallire la build. L'ordine del catalogo è anche l'ordine della lista nelle impostazioni.

Ogni tema definisce una palette completa chiara e una scura con gli stessi ruoli semantici. Non basta sostituire il viola con un altro valore: superfici, testo, stati e accenti devono restare coerenti e leggibili insieme. Le palette complete sono in Rust e le combinazioni principali di testo e azione hanno test di contrasto. Sono stati rivisti screenshot con rendering software di tutti e quattro i temi in entrambe le modalità; la verifica interattiva su desktop resta necessaria.

### Colori di riferimento OpenMove confermati

La nuova immagine di riferimento sostituisce i valori scritti il 14 novembre. L'utente conferma i valori RGB dell'immagine come fonte autorevole, incluso l'arancione:

| Colore | Valore confermato |
| --- | --- |
| Blu | `#4A47F7`, `rgb(74, 71, 247)` |
| Arancione | `#D86F4E`, `rgb(216, 111, 78)` |
| Rosa | `#D38BE7`, `rgb(211, 139, 231)` |
| Giallo | `#F1E600`, `rgb(241, 230, 0)` |
| Verde | `#CBF02C`, `rgb(203, 240, 44)` |
| Bianco | `#FFFFFF`, `rgb(255, 255, 255)` |

L'esadecimale dell'arancione stampato nell'immagine (`#D26F50`) è incoerente con il suo RGB: vale `#D86F4E`. I precedenti blu `#4A45FB`, arancione `#F3704B`, rosa `#F493EA`, giallo `#F9E800` e verde `#B4F125` sono superati.

### Palette OpenMove

Ogni tema OpenMove abbina una palette chiara e una scura, esplorate nella [pagina di prova](./mockup-openmove.html) (sei palette in totale). I valori esatti sono in `palettes.rs`; qui si descrivono i ruoli principali. Il blu `#4A47F7` è il blu di marca; l'arancione di marca resta nei turni di parola in tutte le palette.

| Tema | Chiaro | Scuro |
| --- | --- | --- |
| Bubblegum (`openmove-1`) | **Bubblegum** | **Grafite e blu** |
| Lavanda elettrica (`openmove-2`) | **Bianco e blu** | **Nero e lime** |
| Campo Blu (`openmove-3`) | **Lavanda elettrica** | **Campo blu** |

**Bubblegum (chiaro).** Pagina bianca, schede rosa tenue `#F8ECFB`, azioni blu di marca, titoli e testo accento sulle schede in magenta `#9035AC` (il rosa di marca chiaro non è usato come testo sul bianco).

**Grafite e blu (scuro).** Pagina grafite `#222228`, schede indaco scuro `#2D2C4F`, azioni blu di marca, titoli in rosa di marca `#D38BE7`, focus verde.

**Bianco e blu (chiaro).** Pagina bianca, schede `#EEEEFF`, azioni, titoli e timer in blu di marca, azione distruttiva quasi nera, focus in arancione scuro.

**Nero e lime (scuro).** Pagina nera `#1D1D1B`, schede blu di marca. Le azioni primarie (**Avvia**) sono lime `#CBF02C` con testo scuro; titoli in rosa; il testo accento sulle schede (`surface-accent`) è giallo. Timer bianco.

**Lavanda elettrica (chiaro, tema Campo Blu).** Pagina lavanda `#ECEBFF`, schede bianche, testo blu notte `#15144A`, azioni blu di marca, titoli e testo accento `#3B38E0`.

**Campo blu (scuro).** Pagina blu di marca `#4A47F7`, schede blu più scuro `#3633D6`. Le azioni primarie sono bianche con testo blu; i titoli sono gialli; timer bianco, focus verde.

**Schisaciaccole.** Mantiene l'identità originale viola in entrambe le modalità (marca `#6B2FBF`, fondo scuro `#222228`).

Non usare il rosa chiaro di marca come testo sul bianco senza una variante adatta. Tutte le combinazioni, incluse icone e focus, richiedono verifica del contrasto e prova nei controlli reali.

### Ruoli dei colori

Le palette condividono i token semantici di `AppTheme`, completi per ciascuna modalità:

| Famiglia | Ruoli da coprire |
| --- | --- |
| Superfici | Pagina, pannelli e schede, bordi e separatori. |
| Testo | Principale, secondario, collegamenti e titoli del timer; testi sui diversi fondi colorati; `surface-accent`, il testo accento disegnato sulle schede (avviso partecipanti nelle righe dei meeting, chip dei tasti nell'Aiuto). |
| Marca e azioni | Primario e secondario, relativi testi e fondi attenuati, stati hover e premuto. |
| Interazione | Focus da tastiera, selezione, controlli disabilitati. |
| Stati semantici | Avviso, errore o azione distruttiva, successo dove presente; testo, fondo e bordo necessari. |
| Timer e turni | Timer normale, pausa, soglia di avviso, scadenza, turno attivo e turni in attesa. |
| Decorazioni | Accenti e coriandoli, senza significato di stato. |

Ruoli diversi possono condividere un valore all'interno di un tema, ma devono poter variare separatamente. Le trasformazioni dei colori usate dai controlli vanno verificate negli stati reali; se non garantiscono leggibilità, il tema deve fornire valori espliciti. Il lavoro riguarda i colori: geometrie, tipografia e animazioni non diventano opzioni del tema.

## Scelta dell'utente e modalità

Il menu a tre puntini in alto a destra nelle schermate di **selezione del timer** e **selezione del meeting** contiene la voce **Impostazioni**. Le impostazioni contengono la sezione **Tema**, con due controlli indipendenti:

| Controllo | Opzioni |
| --- | --- |
| Tema | Elenco dei soli nomi dei temi del catalogo (**Schisaciaccole**, **Bubblegum**, **Lavanda elettrica**, **Campo Blu**), in una griglia a due colonne dentro un'area scorrevole; **Indietro** è sempre visibile. |
| Modalità | **Sistema**, **Chiaro**, **Scuro**: sceglie la variante chiara o scura del tema selezionato. |

Non esiste una voce **Predefinito della distribuzione** né una descrizione delle varianti per tema: il predefinito della build è un tema come gli altri. Senza una scelta salvata è attivo il predefinito della build e compare selezionato nell'elenco; continua a seguire il predefinito della build, anche dopo un aggiornamento, finché l'utente non sceglie un tema. Scegliere un tema lo conserva esplicitamente anche se cambia il predefinito della build.

La scelta si applica durante l'uso e viene salvata localmente, senza account e senza dipendenza dal backend. Login e logout non cambiano una preferenza visiva locale.

Con **Sistema**, una variazione del sistema cambia la variante chiara/scura senza cambiare il tema. Cambiare tema non altera la modalità. In assenza di una preferenza, la modalità iniziale è **Sistema**.

### Risoluzione e messaggi

1. Un ID salvato e presente nel catalogo prevale sul predefinito della build.
2. Un'assenza della preferenza usa il tema incorporato nella build. Il valore legacy `theme = "distribution"` è letto come assente, in silenzio.
3. Un tema rimosso dal catalogo (per esempio il vecchio `openmove`) o una preferenza non valida ripiega sul predefinito della build, senza impedire l'avvio; il tema predefinito risulta selezionato e il messaggio spiega il ripiego.

Gli errori sono piccoli testi rossi nell'interfaccia, senza modali, nella sezione **Tema** vicino ai controlli:

- Salvataggio fallito: «Scelta applicata per questa sessione, ma non salvata.» La scelta resta attiva nella sessione; non si promette che sopravviva al riavvio.
- Dato non valido o tema rimosso: «Tema salvato non disponibile. Uso il tema predefinito.»
- Errore di lettura: «Impossibile leggere le preferenze. Uso il tema predefinito.»
- Modalità non valida: «Modalità salvata non valida. Uso la modalità Sistema.»

Una preferenza normalmente assente non è un errore e non mostra un avviso. Un tema non valido conserva una modalità valida, e viceversa.

Il TOML usa le chiavi `theme = "schisaciaccole" | "openmove-1" | "openmove-2" | "openmove-3"` e `mode = "system" | "light" | "dark"`. Il salvataggio scrive solo ID espliciti: se l'utente non ha scelto un tema, la chiave `theme` non viene scritta. Una scrittura sostituisce il file atomicamente tramite un file temporaneo nella stessa directory e conserva eventuali chiavi TOML estranee. Un errore lascia attiva la scelta della sessione e conserva il file precedente. Un file TOML non valido può essere riparato selezionando una scelta valida nelle impostazioni.

Le preferenze locali sono salvate in formato **TOML** nel file **`~/.config/schisaciaccole/config.toml`**, dove `~` indica la home dell'utente. Il percorso è fisso e vale anche per i fork: la coesistenza dell'app originale e di un fork non è considerata uno scenario realistico, quindi non sono richiesti namespace per fork o identificativi di distribuzione. Non serve introdurre un servizio di configurazione o sincronizzare la scelta nelle preferenze dei meeting.

## Predefinito a compilazione

Comportamento confermato per `SCHISACIACCOLE_DEFAULT_THEME`:

| Valore nella build | Risultato |
| --- | --- |
| Variabile assente | `schisaciaccole`, il tema originale. |
| `schisaciaccole`, `openmove-1`, `openmove-2` o `openmove-3` | Tema corrispondente come predefinito. |
| Variabile presente ma vuota, oppure ID sconosciuto (compreso il rimosso `openmove`) | Fallimento della build con messaggio descrittivo e lista degli ID ammessi. |

La variabile appartiene alla compilazione del desktop, come il backend URL; non è una configurazione del server né una lettura dell'ambiente a ogni avvio. La validazione deve usare lo stesso catalogo degli ID disponibili. Il build script dichiara `cargo:rerun-if-env-changed=SCHISACIACCOLE_DEFAULT_THEME` affinché Cargo riconsideri la scelta quando cambia.

Esempio della build per il fork OpenMove, con URL segnaposto:

```sh
SCHISACIACCOLE_BACKEND_URL=https://schisaciaccole.example.com \
SCHISACIACCOLE_DEFAULT_THEME=openmove-1 \
cargo run --release -p schisaciaccole
```

La scelta del tema deve funzionare anche senza `SCHISACIACCOLE_BACKEND_URL`. La configurazione operativa è documentata anche nel README.

## Architettura

Il catalogo dei temi e la risoluzione della preferenza sono implementati in Rust dentro `crates/app`. Il build script valida il predefinito; la gestione locale delle preferenze conserva la scelta dell'utente. Non occorrono modifiche a `crates/core`, API, server o database.

`AppTheme` resta il punto di accesso dei componenti Slint ai colori. Rust comunica il tema scelto; Slint usa i binding per aggiornare i colori senza duplicare la logica nei singoli componenti. La separazione concreta dei file va mantenuta semplice e coerente con i moduli esistenti. I colori scritti direttamente nella UI vanno esaminati per ruolo e collegati ai token appropriati, incluse le decorazioni.

Il catalogo è guidato dai dati ed è estendibile in codice: aggiungere un tema richiede tre interventi in `crates/app/src`: una variante di `ThemeId`, una voce nel `THEME_CATALOG` ordinato di `theme_catalog.rs` (ID stabile e nome; la lista delle impostazioni ne segue l'ordine) e le funzioni delle palette chiara e scura in `palettes.rs`, collegate dalla `match` esaustiva (il compilatore segnala le palette mancanti). File di tema esterni e selettore libero di colori restano fuori dallo scopo. Non si introduce `THEME_FILE`.

## Verifiche

- Build senza variabile, con ciascun ID valido, con valore vuoto e con ID sconosciuto; cambio della variabile senza pulizia manuale della build.
- Avvio iniziale, scelta esplicita, riavvio, assenza di una scelta e aggiornamento che modifica il predefinito. Una scelta esplicita deve restare invariata; chi non ha scelto deve ricevere il nuovo predefinito.
- Preferenza assente, legacy `distribution`, non valida o riferita a un tema rimosso (`openmove`); errore di scrittura con scelta attiva nella sessione e messaggio inline; errore di lettura con ripiego e messaggio distinto confermati.
- Tutti e quattro i temi nelle modalità chiara, scura e di sistema, comprese le variazioni del sistema mentre l'app è aperta.
- Contrasto di testo, icone, bordi funzionali e focus da tastiera su tutti i fondi pertinenti; stati normale, hover, premuto, selezionato e disabilitato.
- Timer normale, pausa, avviso, scadenza, turni e Daily; messaggi di errore, dialoghi, schermata compatta e celebrazioni. Il significato degli stati deve restare riconoscibile senza affidarsi al solo colore.
- Cambio del tema senza modificare timer, partecipanti, navigazione o modalità; funzionamento locale anche senza backend e attraverso login/logout.

I test automatici coprono catalogo e validazione del predefinito, risoluzione e persistenza, errori di lettura/scrittura e contrasti principali. Test con rendering software verificano il fondo effettivo e gli aggiornamenti nativi simulati, anche dopo un override e il ritorno a Sistema, conservando tema e pagina. Gli screenshot con rendering software dei quattro temi in entrambe le modalità sono stati rivisti. Le prove interattive elencate sopra restano criteri di verifica visiva sui desktop supportati; non vanno confuse con la sola compilazione o con i test unitari.
