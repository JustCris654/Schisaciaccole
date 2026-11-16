---
status: proposed
updated: 2026-11-16
---

# Decisioni: aggiornamento automatico

Il design resta `proposed`: le correzioni di revisione non approvano automaticamente le altre scelte ancora aperte.

| Data | Decisione |
| --- | --- |
| 2026-11-16 | Accordo tecnico finale tra agenti: adottare D nella proposta, con `RENAME_SWAP` solo su volume supportato (altrimenti release), vecchia app sorvegliante, figlio `Command`/`Child`, conferma nonce dopo UI pronta entro 30 secondi, kill/wait del solo figlio posseduto prima di rollback. Mai kill dopo exit status da `try_wait`. Lock stabile non ereditato (`CLOEXEC`), nuova app senza lock d’installazione durante handshake; piccolo stato durevole prima dello swap, backup conservato sugli errori. Foreground/TCC/portachiavi da verificare su Mac prima del merge. |
| 2026-11-16 | Rischio residuo della proposta D: se muore il sorvegliante e la nuova app non funziona, backup e stato supportano reinstallazione/ripristino manuale; nessun helper di recovery indipendente nell’MVP. Esito ambiguo all’avvio: sotto lock non pulire. A e Sparkle escluse dall’MVP; questo accordo non equivale ad approvazione utente (`status: proposed`). |
| 2026-11-16 | Secondo clic esplicito "Installa e riavvia" quando idle; con bozze/editor/salvataggi si rinvia alle normali azioni di salvataggio e chiusura, senza cancellazioni né nuova conferma per perdere dati. |
| 2026-11-16 | Cache automatica locale di 6 ore per repository/versione installata nelle directory di configurazione esistenti. Manuale forza rete, sempre con rate limit e worker unico; non si presumono 304 gratuiti senza token. |
| 2026-11-16 | Estrazione Rust streaming confinata tramite parser ZIP validante e scritture relative/no-follow; niente link simbolici nell’MVP, permessi eseguibili preservati. Default 1 GiB estratto e 10.000 entry oltre ai 200 MiB ZIP; validare soglie e politica link sull’asset firmato reale prima del merge. |
| 2026-11-16 | Proposta iniziale di revisione, superata dall’accordo D sopra: helper indipendente e journal per recovery più forte. Il piano finale rinuncia a questa recovery indipendente nell’MVP e dichiara il rischio residuo. |
| 2026-11-16 | Download durante l'uso normale; installazione e riavvio solo idle, con guardia sul thread UI e blocco delle nuove azioni nella fase finale. Timer, editor aperto, bozze non salvate o salvataggi pendenti differiscono l'installazione; nessuna cancellazione automatica delle bozze. |
| 2026-11-16 | Firma Apple: Developer ID, ad-hoc valida senza identità certificata, eventuale unsigned riconosciuto ed errori sono casi distinti; nessun downgrade Developer ID verso ad-hoc/unsigned. Il percorso senza Developer ID esplicitamente riconosciuto mantiene SHA-256; per il percorso Developer ID verificare catena Apple attendibile, Team ID e identificatore firmato oltre a `codesign --verify --strict --deep`. SHA-256 della stessa release resta integrità, senza autenticare una release GitHub compromessa; nessuna firma propria aggiunta. |
| 2026-11-16 | Estrazione confinata con controlli prima/durante l’estrazione e staging privato univoco; soglie ora fissate come default nell’accordo sopra, da validare sul pacchetto reale. Compatibilità Mach-O e macOS verificata prima dello swap. |
| 2026-11-16 | "Controlla aggiornamenti" nel menu solo Manuale/I miei meeting: esito manuale esplicito, errori automatici silenti, cache per repository/versione installata, rispetto di `Retry-After`/reset e nessun worker concorrente. Controlli periodici fuori scope; durata cache precisata dall’accordo sopra. |
| 2026-11-16 | Dopo un errore il pulsante dice "Apri la release", con messaggio di errore separato: l'azione non è nascosta dietro al testo dell'errore. |
| 2026-11-15 | Il pulsante compare solo nella barra di navigazione di **Manuale** e **I miei meeting**. Nelle schermate del timer l'aggiornamento non viene segnalato. |
| 2026-11-15 | L'integrità del download si verifica solo con SHA-256 (`SHA256SUMS` nella release). Una firma propria è rimandata. |
| 2026-11-15 | Su Linux (`.deb`, Arch) il pulsante apre la pagina della release. |
| 2026-11-15 | Niente AppImage per ora: l'installazione sul posto vale solo per macOS. |
| 2026-11-15 | Le release si leggono dall'API pubblica di GitHub senza token: il repository è pubblico. |
