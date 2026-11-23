---
status: proposed
updated: 2026-11-16
---

# Domande aperte: aggiornamento automatico

## Già chiarite

Le risposte del 2026-11-16 registrano l’accordo tecnico finale tra agenti; il design resta `proposed` fino all’approvazione dell’utente.

- [x] Quale alternativa di installazione scegliere: A (helper indipendente/journal/conferma sana), D (swap nativo e vecchia app sorvegliante), oppure approfondire un framework come Sparkle? D non garantisce recovery se muore il sorvegliante e la nuova app non parte: l'utente accetta questo rischio residuo rispetto ad A, o richiede la recovery indipendente? **Risposta finale concordata:** Accordo tecnico tra agenti su D: swap nativo e vecchia app sorvegliante con processo figlio posseduto e conferma sana; rischio watcher morto/nuova guasta dichiarato, backup e ripristino manuale. Nessun helper indipendente MVP. Resta proposta fino all’approvazione utente. (2026-11-16)
- [x] Quando l’app è idle e l’aggiornamento è pronto, installare/riavviare automaticamente (proposta da rivalutare), o richiedere il pulsante "Installa e riavvia"? **Risposta finale concordata:** Proposta finale concordata: secondo clic esplicito "Installa e riavvia", con guardia UI idle. (2026-11-16)
- [x] Quale UX usare quando esistono bozze: semplice rinvio o conferma con eventuale salvataggio? Come riprendere l'aggiornamento dopo il salvataggio? Non è autorizzata la cancellazione automatica. **Risposta finale concordata:** MVP: semplice rinvio; l’utente salva e chiude normalmente e poi preme "Installa e riavvia". Nessuna cancellazione o conferma per perdere bozze. (2026-11-16)
- [x] Quali durata, persistenza e invalidazione assegnare alla cache per repository/versione installata? Come presentare il risultato manuale servito dalla cache o rinviato per `Retry-After`/reset, senza aggirare i limiti? **Risposta finale concordata:** Default automatico 6 ore, chiave repository/versione installata, piccolo file nelle directory di configurazione esistenti. Manuale forza rete ma rispetta rate limit e worker unico; esito di rinvio esplicito, nessuna promessa di 304 gratuiti. (2026-11-16)
- [x] Quali soglie per dimensione totale estratta e numero di file, e quali strumenti di ispezione/estrazione garantiscono i vincoli prima e durante l'estrazione? **Risposta finale concordata:** Default 1 GiB estratto e 10.000 entry, ZIP massimo 200 MiB; parser ZIP Rust validante, preflight e controlli streaming per scrittura relativa/no-follow, niente symlink MVP e permessi eseguibili preservati. Validare asset firmato prima del merge; se necessita link rivalutare prima del merge. (2026-11-16)
- [x] Come distribuire e rendere rilanciabile il recovery esterno anche se la nuova app non parte o il bundle manca? **Risposta finale concordata:** Non previsto nell’MVP D: file di stato minimo durevole e backup, vecchia app sorvegliante; se muore e nuova app è guasta resta reinstallazione/ripristino manuale. All’avvio stato ambiguo conservato sotto lock. (2026-11-16)
- [x] Quale protocollo e timeout per la conferma esplicita di avvio sano, dopo inizializzazione app/UI? Come diagnosticare e guidare il recupero se la nuova istanza rimane viva senza confermare? `open` riuscito non basta e il backup resta disponibile. **Risposta finale concordata:** Conferma nonce di transazione dopo UI pronta, default 30 secondi. Processo figlio posseduto via Child: se vivo kill/wait prima di rollback; dopo exit status da try_wait mai kill. Errori di terminazione/rollback conservano backup e stato. Lock CLOEXEC e nessuna acquisizione del lock d’installazione nel figlio durante handshake. (2026-11-16)

- [x] Le bozze non salvate dell'editor si perdono con il riavvio. Va bene così (proposta), oppure serve una conferma quando esistono bozze? La perdita automatica non è autorizzata: presenza di bozze, editor aperto o salvataggi pendenti differisce installazione/riavvio. L’MVP rinvia alle normali azioni di salvataggio/chiusura e al successivo clic esplicito, come precisato sopra. (2026-11-16)
- [x] Clic dopo un errore: aprire la pagina della release (proposta) o riprovare l'installazione? Aprire la release con pulsante esplicito "Apri la release" e messaggio di errore separato. (2026-11-16)

- [x] Il repository GitHub è pubblico? Sì: API senza token. (2026-11-15)
- [x] Quali piattaforme si aggiornano da sole? Solo macOS; AppImage rimandato. (2026-11-15)
- [x] Cosa fa il pulsante con `.deb` e Arch? Apre la pagina della release. (2026-11-15)
- [x] Come si verifica il download? Solo SHA-256 per ora. (2026-11-15)
- [x] Dove si vede il pulsante? Solo in **Manuale** e **I miei meeting**, mai nel timer. (2026-11-15)

## Da decidere

- [ ] Attivare l'updater solo con `SCHISACIACCOLE_UPDATE_REPO` impostata in CI (proposta), o sempre con il repository scritto nel codice?
- [ ] Testo e posizione del pulsante: "Aggiorna a vX.Y.Z" nella barra, a sinistra dei tre puntini (proposta)?
- [ ] Come provare il flusso su macOS prima del merge: release di prova su un fork (proposta) o un tag di prerelease nel repository principale (ma `releases/latest` ignora le prerelease)?

## Verifiche prima del merge

- [ ] Smoke su Mac: swap supportato/fallback, processo figlio e timeout/rollback, conferma UI, foreground/TCC/portachiavi, lock `CLOEXEC` non ereditato e nessun kill dopo raccolta del figlio.
- [ ] Asset firmato reale: validare default 1 GiB/10.000 entry, ZIP leciti e permessi eseguibili; se il bundle richiede link, rivalutare estrazione prima del merge.
- [ ] Scegliere in implementazione dipendenze ZIP/Mach-O/API native necessarie, senza cambiare i requisiti concordati.
