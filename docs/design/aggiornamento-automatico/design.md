---
status: proposed
updated: 2026-11-16
---

# Aggiornamento automatico dell'app desktop

## Problema

Oggi l'app desktop non sa quando esce una versione nuova. Chi la usa deve accorgersene da solo, andare sulla pagina delle release di GitHub, scaricare il pacchetto giusto e reinstallarlo. Nella pratica molte installazioni restano indietro, e dato che app e server si rilasciano insieme (vedi [preferenze dei meeting](../preferenze-meeting/design.md#compatibilità)) un'app vecchia può smettere di funzionare con il backend aggiornato.

## Obiettivi

1. All'avvio l'app controlla su GitHub Releases se esiste una versione più recente di quella in esecuzione.
2. Se esiste, compare un pulsante di aggiornamento, visibile **solo** nelle schermate **Manuale** e **I miei meeting**. Nelle schermate del timer non compare niente.
3. Su **macOS** il pulsante scarica la nuova versione, verifica lo SHA-256, sostituisce il bundle `.app` installato e riavvia l'app.
4. Su **Linux** (`.deb`, Arch) il pulsante apre nel browser la pagina della release. Lo stesso succede su macOS quando l'installazione sul posto non è possibile.
5. Il controllo è attivo solo nelle build di release prodotte dalla CI. Una build locale non controlla niente.

## Fuori dallo scopo

- **AppImage**: oggi non viene prodotto. Il supporto (job in CI, sostituzione del file indicato da `$APPIMAGE`, riavvio) è rimandato.
- Sostituire i file installati da `.deb` o pacman: li gestisce il package manager.
- Firma degli aggiornamenti con chiave propria (minisign/ed25519): per ora solo SHA-256. Su macOS la firma Apple del bundle viene comunque verificata (vedi [Verifiche](#verifiche-del-bundle-scaricato)).
- Controlli periodici mentre l'app è aperta: controllo automatico solo all'avvio; resta disponibile il controllo manuale dal menu.
- Canali beta: le prerelease e le bozze vengono ignorate.
- macOS Intel: la release produce solo `macos-arm64`. Su un Mac Intel il pulsante apre la pagina della release.

## Comportamento per l'utente

### Controllo all'avvio

- Subito dopo l'apertura della finestra parte un controllo in background. Non blocca la UI, non mostra caricamenti e non dipende dalla sessione: funziona anche nella modalità solo locale.
- Se fallisce (rete assente, limite di richieste di GitHub, risposta non valida) non compare niente. L'errore finisce solo nel log (`warn`).
- Se la versione pubblicata è uguale o più vecchia, non compare niente.
- Nel menu delle sole schermate **Manuale** e **I miei meeting** è disponibile "Controlla aggiornamenti". L'esito manuale è esplicito anche quando non ci sono aggiornamenti o il controllo fallisce; gli errori del controllo automatico restano silenti.
- Cache e richieste in corso sono condivise per repository e versione installata. Un controllo manuale non avvia un worker concorrente e rispetta `Retry-After` e il reset del limite GitHub, anche su IP condivisi. Il controllo automatico usa una cache di 6 ore per repository/versione installata, persistita in un piccolo file locale nelle directory di configurazione già usate dall’app. Il comando manuale forza un controllo di rete, sempre rispettando rate limit e worker unico; se rinviato lo segnala esplicitamente. Non si presume che le risposte 304 siano gratuite nel flusso senza token.

### Pulsante

- Si trova nella barra di navigazione (componente `Navigation`), che esiste solo sulle pagine **Manuale** e **I miei meeting**. È a destra, prima del pulsante del menu a tre puntini.
- Testo: "Aggiorna a vX.Y.Z". Usa lo stile delle azioni primarie del tema attivo.
- Quando la barra è stretta (`narrow`) il testo si accorcia in "Aggiorna". Il nome accessibile resta "Aggiorna a vX.Y.Z".

### Clic su macOS, installazione possibile

| Stato | Pulsante | Note |
| --- | --- | --- |
| Controllo manuale | "Controllo…", disattivato | Esito esplicito nel menu; nessun worker concorrente. |
| Download | "Download… 42%", disattivato | Percentuale da `Content-Length`; senza lunghezza solo "Download…". L'app resta utilizzabile. |
| Pronto | "Installa e riavvia" (secondo clic esplicito della proposta concordata), attivo solo quando idle | `ReadyToInstall` è visibile solo nelle due schermate home. Installazione differita finché ci sono timer, editor, bozze o salvataggi pendenti. |
| Installazione | "Installazione…", disattivato | Guardia sul thread UI, blocco delle nuove azioni, swap sorvegliato dopo estrazione/verifiche. |
| Riavvio | vecchia finestra nascosta | La vecchia app resta viva come sorvegliante, con finestra nascosta solo nella fase finale, fino alla conferma della nuova. |
| Errore | "Apri la release", attivo | Messaggio "Aggiornamento non riuscito" separato dal pulsante; dettaglio nel log. L'azione è esplicita. |

Il download non richiede una conferma e consente il normale uso dell'app, compreso avviare un timer o aprire l'editor. Questo non autorizza il riavvio: la presenza della navigazione al clic iniziale non garantisce che l'app sia ancora idle al termine del download.

Installazione e riavvio sono ammessi solo senza timer attivo, editor aperto, bozze non salvate e salvataggi pendenti. Il thread UI rivaluta insieme queste condizioni prima della fase finale e impedisce nuove azioni che le invalidino fino alla conferma sana e all’uscita della vecchia app, oppure al ripristino/annullamento della fase finale. Non si eliminano bozze automaticamente. Nell’MVP non si introduce una conferma che autorizzi a perderle: si rinvia e si lascia all’utente chiudere l’editor e salvare le bozze con le azioni normali, poi premere "Installa e riavvia" quando idle.

### Clic negli altri casi

Il pulsante apre la pagina della release (`html_url`) con il browser predefinito, come fa il login. L'app resta aperta. Succede:

- su Linux;
- su macOS Intel;
- su macOS quando l'app non gira da un bundle `.app` (ad esempio `cargo run`);
- su macOS con App Translocation (il percorso dell'eseguibile contiene `/AppTranslocation/`): l'app è stata aperta senza spostarla e il bundle è in sola lettura;
- su macOS quando la cartella che contiene il bundle non è scrivibile (ad esempio `/Applications` per un utente non amministratore);
- su macOS quando la release non ha lo `.zip` o lo `SHA256SUMS` attesi;
- quando il bundle candidato non supporta l’architettura o la versione macOS in uso, senza eseguire lo scambio.

Nei primi casi il testo resta "Aggiorna a vX.Y.Z": l'utente non vede differenze finché non clicca.

## Build e configurazione

### Variabile di compilazione

| Variabile | Valore | Effetto |
| --- | --- | --- |
| `SCHISACIACCOLE_UPDATE_REPO` | `owner/name`, ad esempio `JustCris654/Schisaciaccole` | Repository GitHub da cui leggere le release. |

- Si legge con `option_env!` come `SCHISACIACCOLE_BACKEND_URL`.
- Assente o vuota: l'updater è spento, non parte nessuna richiesta e il pulsante non compare mai.
- Un valore che non ha la forma `owner/name` (un solo `/`, caratteri `[A-Za-z0-9._-]`) fa fallire la build, come un tema predefinito sconosciuto.
- Nella CI di release vale `${{ github.repository }}`, quindi un fork pubblica e controlla le proprie release.

Motivo: un'app compilata in locale con un altro `SCHISACIACCOLE_BACKEND_URL` non deve "aggiornarsi" alla build ufficiale, che ha un backend diverso.

### Cambiamenti alla release

- I job `deb`, `arch` e `macos` passano `SCHISACIACCOLE_UPDATE_REPO` alla build (per Arch anche attraverso `sudo env`, come il backend).
- Il job `release`, prima di `gh release create`, genera `dist/SHA256SUMS` con `sha256sum` di tutti gli asset (formato `<hex>  <nome>`) e lo allega alla release.
- I nomi degli asset non cambiano: l'updater cerca `schisaciaccole-<versione>-macos-arm64.zip`, che contiene `Schisaciaccole.app` in radice (`ditto --keepParent`).

## Scelta di installazione concordata

Il confronto con il thread autore «Auto Update Desktop App» converge su **D: swap atomico nativo e vecchia app come sorvegliante**, come proposta finale tra agenti. Il design resta `proposed` fino all’approvazione dell’utente. Il secondo clic "Installa e riavvia" avvia la fase finale solo quando idle.

A (helper indipendente, journal e recovery esterno) e Sparkle restano esclusi dall’MVP per mantenere limitata l’implementazione; non si sostiene che D offra la stessa recovery indipendente di A. Se muore il sorvegliante e la nuova app non funziona, resta il ripristino/reinstallazione manuale con backup conservato. L’installazione manuale è anche il ripiego quando lo swap non è supportato.

## Flusso tecnico

### Controllo

1. Worker `update-check` (stesso schema di `spawn` in `session.rs`).
2. `GET https://api.github.com/repos/<repo>/releases/latest` con:
   - `Accept: application/vnd.github+json`;
   - `User-Agent: schisaciaccole/<versione>` (GitHub lo richiede; nessun dato personale);
   - timeout di 10 secondi, senza token.
   L'endpoint esclude bozze e prerelease. Il limite senza autenticazione è condiviso per IP: non si presume sufficiente. Si rispettano `Retry-After` e reset del limite, con cache automatica di 6 ore per repository/versione installata e un solo worker attivo. Il comando manuale forza la rete ma non aggira `Retry-After`/reset.
3. Dalla risposta servono `tag_name`, `html_url` e `assets[].{name, browser_download_url, size}`.
4. Versione: `tag_name` senza la `v` iniziale, letto con il crate `semver` e confrontato con `CARGO_PKG_VERSION`. Un tag non leggibile equivale a "nessun aggiornamento".
5. `html_url` e ogni `browser_download_url` usati devono iniziare con `https://github.com/<repo>/releases/`. Altrimenti il risultato viene scartato.
6. Il worker manda un evento al thread della UI con versione, `html_url` e, su macOS arm64, gli URL dello `.zip` e di `SHA256SUMS`.

### Installazione su macOS

1. **Bundle attuale e lock**: `std::env::current_exe()` canonicalizzato identifica l'antenato `*.app` che contiene `Contents/MacOS/<exe>`. Se manca, è translocato o la destinazione non è scrivibile, si apre la release. Un lock esclusivo stabile per percorso canonico del bundle coordina tutte le istanze e resta valido durante lo swap. Il descrittore del lock non è ereditato dal processo figlio (`CLOEXEC`); la nuova app non acquisisce il lock d’installazione prima della fine dell’handshake.
2. **Staging e supporto swap**: directory privata univoca accanto al bundle, senza riusare directory preesistenti e sullo stesso volume. Verificare disponibilità di `renamex_np` con `RENAME_SWAP` e supporto del volume ([Apple: volumeSupportsSwapRenamingKey](https://developer.apple.com/documentation/foundation/urlresourcekey/volumesupportsswaprenamingkey)); altrimenti proporre la release. Lo scambio dei due percorsi è atomico quando supportato; avvio, conferma e pulizia non sono un’unica operazione atomica.
3. **Download**: `SHA256SUMS` (limite 64 KiB), poi ZIP in streaming nello staging (limite 200 MiB e confronto con `size` dichiarata). Avanzamento al massimo una volta per punto percentuale, sconosciuto senza lunghezza. reqwest segue il redirect verso il CDN di GitHub. Durante il download l’app resta utilizzabile.
4. **SHA-256**: calcolato durante lo streaming con `sha2`, deve coincidere con la riga dello ZIP in `SHA256SUMS`. Riga mancante/ambigua o hash diverso sono errori. È integrità rispetto al manifest della stessa release, non autenticità contro una compromissione GitHub.
5. **Estrazione Rust confinata**: parser ZIP validante con preflight delle entry e controlli durante ogni scrittura in streaming, tramite API relative alla directory di staging e senza seguire link (`no-follow`). Rifiutare percorsi assoluti, componenti `..`, link simbolici nell’MVP e ogni scrittura fuori staging. Il parser deve gestire correttamente i formati ZIP leciti, inclusi data descriptor: non si richiede ingenuamente l’uguaglianza delle dimensioni negli header locali e centrali. Default: massimo **1 GiB estratto** e **10.000 entry**, conteggiati anche durante lo streaming; preservare i permessi eseguibili. Soglie e assenza di link devono essere validate sull’asset firmato reale: se richiede link, rivalutare la politica prima del merge. Un solo bundle atteso `Schisaciaccole.app`; controlli solo dopo l’estrazione non bastano. Nessun uso di `ditto` basato su soli controlli pre/post.
6. **Validazione e guardia UI**: completare verifiche firma, metadati e compatibilità prima dello swap. Con download pronto, l’utente preme "Installa e riavvia"; il thread UI rivaluta timer/editor/bozze/salvataggi e blocca nuove azioni nella fase finale. La vecchia app rimane viva come sorvegliante e nasconde la finestra solo in questa fase; se si annulla/ripristina torna utilizzabile.
7. **Stato minimo durevole**: prima dello swap persistere un piccolo file di stato con percorsi del bundle e del candidato/backup, versioni e identificatore della transazione, insieme alla fase necessaria a interpretare l’esito. Non è un database di journal. Il file deve essere durevole prima della mutazione e aggiornato prima di rollback/pulizia. Dopo lo swap il percorso del candidato contiene il vecchio bundle e funge da backup.
8. **Swap e avvio diretto**: eseguire `RENAME_SWAP`, quindi avviare l’eseguibile del nuovo bundle con `Command` come processo figlio; mantenere l’handle `Child`, senza reaping prima della terminazione. Non ricercare processi per PID/nonce. L’avvio diretto bypassa LaunchServices: foreground, TCC e portachiavi vanno verificati su un Mac come condizione prima del merge. Un errore di spawn avvia il rollback, senza perdere backup/stato.
9. **Conferma sana**: la nuova app invia una conferma associata al nonce della transazione solo dopo inizializzazione e UI pronta, senza acquisire il lock d’installazione detenuto dal sorvegliante. Default attesa **30 secondi**. Il successo dello spawn, come quello di `/usr/bin/open`, non dimostra avvio sano. Il nonce correla la conferma, non autorizza a cercare o terminare processi estranei.
10. **Timeout, uscita e rollback**: senza conferma valida, il sorvegliante termina solo il processo figlio posseduto con `Child::kill`, se non già terminato, e attende con `wait` la sua uscita prima di invertire lo swap. Se `Child::try_wait` restituisce un exit status, il figlio è già raccolto: da quel momento non si invia mai kill su quel `Child`/PID e si passa al rollback. Solo dopo uscita sicura eseguire lo swap di rollback. Errori di terminazione, attesa o rollback conservano backup e stato, senza pulizia distruttiva e senza sostituzioni mentre il processo può ancora essere vivo.
11. **Successo e pulizia**: una conferma valida permette di registrare il successo e pulire backup/staging; solo dopo la vecchia app esce. Errori di pulizia conservano lo stato necessario e non invalidano la nuova app sana.
12. **Crash e rischio residuo**: nessun helper di recovery indipendente nell’MVP. Se il sorvegliante muore e la nuova app non parte o è guasta, la recovery automatica non è garantita: restano backup/stato e reinstallazione/ripristino manuale. A ogni avvio, sotto lock, riconciliare lo stato con il filesystem e non pulire quando l’esito è ambiguo; un lock libero da solo non prova che il recupero sia sicuro. Una nuova app conferma salute solo dopo UI pronta. Nessuna scansione di PID supplisce al sorvegliante morto.

### Verifiche del bundle scaricato

- `Contents/Info.plist` (letto con `plutil -extract ... raw`): `CFBundleIdentifier` uguale a quello attuale (`com.openmove.schisaciaccole`) e `CFBundleShortVersionString` uguale alla release. Questi metadati non provano l'identità del firmatario.
- Compatibilità prima dello swap: architetture Mach-O dell'eseguibile e degli elementi necessari, più versione minima macOS dichiarata nel plist e nei load command del binario se necessario. Rifiutare un candidato incompatibile con macchina e sistema in uso e offrire la pagina della release.
- Firma: classificare esplicitamente **Developer ID**, **ad-hoc senza identità certificata**, eventuale **unsigned riconosciuto**, oppure **errore** di lettura/verifica. L'assenza di `TeamIdentifier` non equivale né a errore né a identità Developer ID. Nessun errore degrada al percorso SHA-256 meno vincolante.
  - Se il bundle attuale è firmato Developer ID, il candidato deve superare `codesign --verify --strict --deep` **e** un controllo esplicito del requisito di trust: catena Apple/Developer ID attendibile, stesso Team ID e stesso identificatore **firmato** attesi. Il solo Team ID o `Info.plist` non bastano. Firma diversa, non trusted o non leggibile blocca lo swap; nessun downgrade da Developer ID ad ad-hoc o unsigned.
  - Su Apple silicon il codice nativo richiede almeno firma ad-hoc: una build senza secret Apple può quindi avere una firma ad-hoc valida, senza identità certificata, anziché essere unsigned ([Apple: App code signing process in macOS](https://support.apple.com/en-sa/guide/security/secebb113be1/web)). Una firma ad-hoc valida esplicitamente riconosciuta nel percorso senza Developer ID può usare il controllo SHA-256 concordato, come l'eventuale unsigned esplicitamente riconosciuto dove eseguibile. L'hash non autentica il publisher; classificazione ed errori vanno testati separatamente.
  - Riferimento per requisiti e verifica della firma: [Apple TN2206 — macOS Code Signing In Depth](https://developer.apple.com/library/archive/technotes/tn2206/_index.html).
- Quarantena e notarizzazione non sostituiscono queste verifiche; l'assenza di `com.apple.quarantine` non è una prova di attendibilità o di avvio sano.

### Portachiavi

Il refresh token sta nel portachiavi di macOS. Per le build Developer ID l'accesso segue il requisito di firma (stesso Team ID e identificatore), quindi resta valido dopo l'aggiornamento. Per le build ad-hoc o eventualmente unsigned macOS può chiedere di nuovo il permesso alla prima lettura, oppure l'app può chiedere un nuovo login. È il comportamento previsto dalla proposta, da verificare sul Mac di prova.

## Struttura del codice

- `crates/app/src/updater.rs`:
  - configurazione da `SCHISACIACCOLE_UPDATE_REPO`;
  - worker di controllo;
  - funzioni pure con test unitari: lettura del tag, confronto delle versioni, scelta degli asset, controllo degli URL, lettura di `SHA256SUMS`.
- `crates/app/src/updater/macos.rs` (`#[cfg(target_os = "macos")]`): adattatori di filesystem/processi/firma/compatibilità. Le transizioni di swap, sorveglianza del figlio, file di stato e rollback usano operazioni simulate testabili su tutte le piattaforme.
- Eventi verso la UI con `slint::invoke_from_event_loop`, separati dagli `AppEvent` della sessione, perché l'updater funziona anche senza backend.
- Stato in `AppState`: `UpdateState { Hidden, Checking, Available, Downloading(Option<u8>), ReadyToInstall, Installing, Failed }` più versione e URL. Renderizzato in proprietà Slint della finestra e passato a `Navigation`.
- `Navigation` riceve `update-label`, `update-enabled`, `update-visible` e il callback `update()`.
- Dipendenze da introdurre: `semver` e quelle necessarie per ZIP confinato, ispezione Mach-O e API native di swap/processo; scegliere le librerie nell’implementazione senza fissare qui `cap-std` o altri strumenti. reqwest (blocking, rustls), `sha2`, `serde_json` e `webbrowser` ci sono già.
- La logica non è di dominio Scrum, quindi resta nell'app e non va in `crates/core`.

## Test

- Unitari su tutte le piattaforme: versioni (uguale, vecchia, nuova, tag non valido, prerelease), asset, URL fuori repository, `SHA256SUMS` malformato/ambiguo/mancante, bundle normale/translocato/assente, cache per repository/versione, rate limit e comando manuale senza worker concorrenti.
- Transizioni e fault injection con operazioni simulate cross-platform: rete interrotta, disco pieno, swap non supportato/fallito e rollback fallito, spawn del figlio fallito, hang prima della conferma, timeout 30 secondi, kill/wait falliti o figlio già terminato, crash prima/dopo swap e stato ambiguo, conferma sana assente/presente e pulizia, istanze concorrenti, lock/CLOEXEC (test esplicito che il descrittore non sia ereditato e non falsifichi la presenza del sorvegliante), handshake senza deadlock e divieto di kill dopo exit status da `try_wait`. Verificare conservazione del backup se muore il sorvegliante; nessuna promessa di recovery automatica in quel caso.
- UI/lifecycle: timer avviato, editor aperto, bozze create o salvataggio pendente durante il download; la guardia UI differisce l'installazione e blocca le nuove azioni solo nella fase finale. Nessuna bozza viene persa automaticamente. Progresso senza `Content-Length`, errore separato da "Apri la release", risultato manuale esplicito e rete all'avvio silente.
- Input non fidati: archivio invalido, traversal/percorso assoluto, link simbolici rifiutati, ZIP leciti con data descriptor, permessi eseguibili, numero entry e dimensione estratta oltre limite, Developer ID/ad-hoc/unsigned esplicitamente riconosciuti, downgrade Developer ID vietato, firma diversa/non trusted/non leggibile, architettura o macOS incompatibili. Gli adattatori reali macOS richiedono anche verifica sul sistema operativo.
- Il job attuale `macos-check` in `.github/workflows/ci.yml` esegue check/test, **non clippy**. Il futuro lavoro CI deve aggiungere clippy macOS; questo design non modifica il workflow.
- Smoke test su un Mac vero prima del merge: release X verso X+1 (anche su un fork), download, swap, avvio sano e conferma, timeout con kill/wait e rollback, backup/stato dopo crash del sorvegliante, foreground/TCC/portachiavi e fallback (destinazione non scrivibile, translocation, incompatibilità, volume senza swap). Validare asset firmato reale contro limiti di estrazione e politica senza link prima del merge. I test simulati su Linux non sostituiscono questa prova.
