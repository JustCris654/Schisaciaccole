# Glossario — app

Crate `schisaciaccole` (`crates/app`): app desktop Slint. Aggiornato sul codice basato sul commit `55228bd` del branch `v2`.

## crates/app/src/appearance.rs

| Nome | Tipo | Riga | Descrizione |
| --- | --- | --- | --- |
| `ReducedMotionObserver` | struct | 9 | Mantiene attiva l’osservazione nativa della preferenza di movimento ridotto |
| `observe_reduced_motion` | fn | 20 | Fornisce la preferenza iniziale e gli aggiornamenti nativi; falso se la piattaforma o il servizio non sono disponibili |
| `platform::APPEARANCE_NAMESPACE` | const | 118 | Namespace delle preferenze grafiche del portale XDG su Linux |
| `platform::REDUCED_MOTION_KEY` | const | 119 | Chiave XDG della preferenza di movimento ridotto |
| `platform::Observer (Linux)` | struct | 121 | Guardia del canale che annulla il worker quando viene rilasciata |
| `platform::observe (Linux)` | fn | 126 | Avvia un worker asincrono cancellabile per il portale senza bloccare la UI |
| `platform::observe_portal` | fn | 158 | Si iscrive a SettingChanged, legge il valore iniziale e inoltra gli aggiornamenti |
| `platform::read_preference` | fn | 198 | Legge ReadOne e ripiega sul metodo legacy Read; interpreta i valori sconosciuti come falso |
| `platform::preference_from_value` | fn | 209 | Decodifica le varianti D-Bus anche annidate; soltanto il valore u32 1 abilita il movimento ridotto |
| `platform::changed_preference` | fn | 221 | Decodifica i segnali relativi al movimento ridotto e ignora le altre preferenze |
| `platform::Observer (macOS)` | struct | 38 | Mantiene la registrazione delle notifiche NSWorkspace sul thread principale |
| `platform::Registration` | struct | 43 | Conserva il centro notifiche macOS e il token necessario alla rimozione |
| `Drop for platform::Observer (macOS)` | trait impl | 48 | Rimuove la registrazione macOS quando la guardia viene rilasciata |
| `platform::Observer::drop (macOS)` | metodo | 50 | Rimuove dal centro notifiche il token mantenuto dalla registrazione |
| `platform::observe (macOS)` | fn | 65 | Registra le notifiche sulla coda principale prima di leggere la preferenza NSWorkspace |
| `platform::Observer (altre piattaforme)` | struct | 101 | Guardia senza risorse per le piattaforme prive di integrazione nativa |
| `platform::observe (altre piattaforme)` | fn | 104 | Fornisce falso sulle piattaforme non supportate |

## crates/app/src/backend.rs

| Nome | Tipo | Riga | Descrizione |
| --- | --- | --- | --- |
| `CONNECT_TIMEOUT` | const | 22 | Timeout di connessione di ogni richiesta: 5 secondi |
| `REQUEST_TIMEOUT` | const | 24 | Timeout totale di ogni richiesta: 15 secondi |
| `REFRESH_MARGIN` | const | 26 | Un access token in scadenza entro 30 secondi viene rinnovato prima dell'uso |
| `BackendError` | enum | 30 | Errore di una chiamata al backend |
| `AccessToken` | struct | 65 | Access token tenuto in memoria insieme alla scadenza |
| `BackendClient` | struct | 71 | Client dell'API backend, non thread-safe: da usare da un solo thread alla volta |
| `BackendClient::new` | metodo | 85 | Crea un client per il backend a `base_url`, ignorando gli slash finali |
| `BackendClient::login_url` | metodo | 108 | Costruisce l'URL da aprire nel browser per avviare il login |
| `BackendClient::has_session` | metodo | 119 | Vero se è salvato un refresh token; un errore dello store vale come nessuna sessione |
| `BackendClient::health` | metodo | 130 | Verifica che il backend sia attivo (`GET /healthz`) |
| `BackendClient::exchange_code` | metodo | 145 | Scambia il codice di login con i token e salva il refresh token |
| `BackendClient::fetch_me` | metodo | 167 | Recupera l'utente autenticato |
| `BackendClient::fetch_meetings` | metodo | 179 | Recupera le riunioni tra `from` e `to` |
| `BackendClient::logout` | metodo | 197 | Revoca la sessione sul backend e la dimentica in locale |
| `BackendClient::url` | metodo | 209 | Concatena l'URL di base del backend con un percorso |
| `BackendClient::post_json` | metodo | 213 | Invia una richiesta POST con corpo JSON al percorso indicato |
| `BackendClient::accept_tokens` | metodo | 218 | Tiene l'access token in memoria e salva il refresh token, eventualmente ruotato |
| `BackendClient::clear_session` | metodo | 229 | Dimentica la sessione in memoria e nello store; un errore dello store viene solo registrato |
| `BackendClient::ensure_access` | metodo | 237 | Restituisce un access token valido, rinnovandolo se manca o sta per scadere |
| `BackendClient::refresh` | metodo | 248 | Rinnova l'access token con il refresh token salvato, che ruota a ogni uso |
| `BackendClient::refresh_failure` | metodo | 265 | Interpreta un rinnovo fallito: sessione morta o ri-autenticazione Google svuotano lo store |
| `BackendClient::api_failure` | metodo | 274 | Interpreta una chiamata API fallita: una richiesta di ri-autenticazione Google svuota lo store |
| `BackendClient::request_once` | metodo | 282 | Un tentativo di richiesta autenticata con metodo, query e corpo JSON opzionali |
| `BackendClient::send_authenticated` | metodo | 303 | Richiesta autenticata: rinnova il token se serve, ritenta una volta su 401 `unauthorized`, mappa gli errori |
| `BackendClient::json_request` | metodo | 325 | Richiesta autenticata con risposta JSON decodificata |
| `BackendClient::get_json` | metodo | 337 | GET autenticata con risposta JSON, tramite `json_request` |
| `has_code` | fn | 348 | Vero se `error` è un errore del server con il codice API indicato |
| `send` | fn | 353 | Invia una richiesta trasformando errori di connessione e stati non riusciti in errori |
| `error_from` | fn | 363 | Costruisce l'errore di una risposta non riuscita; il 429 diventa `RateLimited` |
| `decode` | fn | 379 | Decodifica il corpo JSON della risposta, mappando il fallimento su `BackendError::Decode` |

## crates/app/src/config.rs

| Nome | Tipo | Riga | Descrizione |
| --- | --- | --- | --- |
| `backend_url` | fn | 7 | URL base del backend fissato in compilazione da `SCHISACIACCOLE_BACKEND_URL` |
| `normalize_backend_url` | fn | 15 | Toglie spazi e slash finali da un URL del backend |

## crates/app/src/login.rs

| Nome | Tipo | Riga | Descrizione |
| --- | --- | --- | --- |
| `CALLBACK_PATH` | const | 16 | Percorso sul listener loopback a cui il backend reindirizza il browser |
| `POLL_INTERVAL` | const | 18 | Ogni 500 ms l'attesa della callback controlla il flag di annullamento |
| `SUCCESS_MESSAGE` | const | 21 | Messaggio mostrato nel browser quando la callback porta un codice valido |
| `FORBIDDEN_DOMAIN_MESSAGE` | const | 23 | Messaggio per account fuori dai domini consentiti |
| `REAUTH_MESSAGE` | const | 25 | Messaggio per quando Google richiede di concedere di nuovo l'accesso al calendario |
| `RATE_LIMITED_MESSAGE` | const | 27 | Messaggio per quando il backend risponde 429 |
| `UNREACHABLE_MESSAGE` | const | 29 | Messaggio per quando il backend non è raggiungibile |
| `FAILED_MESSAGE` | const | 31 | Messaggio per qualsiasi altro fallimento |
| `LoginError` | enum | 35 | Errore di un tentativo di login |
| `Callback` | enum | 70 | Query di una richiesta al percorso di callback |
| `parse_callback` | fn | 93 | Analizza la query string (senza `?`) di una richiesta di callback |
| `check_callback` | fn | 123 | Verifica una callback analizzata rispetto allo `state` inviato e ne restituisce il codice |
| `callback_page` | fn | 138 | Costruisce la pagina HTML UTF-8 mostrata nel browser a fine login |
| `login_error_message` | fn | 153 | Messaggio per l'utente su un login fallito |
| `backend_error_message` | fn | 165 | Messaggio per l'utente su una chiamata al backend fallita |
| `error_code_message` | fn | 178 | Messaggio per l'utente a partire da un codice di errore del backend (`forbidden_domain`, ...) |
| `LoopbackServer` | struct | 187 | Listener HTTP su `127.0.0.1` che attende il reindirizzamento a fine login nel browser |
| `LoopbackServer::bind` | metodo | 198 | Si mette in ascolto su `127.0.0.1` su una porta scelta dal sistema operativo |
| `LoopbackServer::port` | metodo | 212 | Porta su cui è in ascolto il listener |
| `LoopbackServer::wait_for_callback` | metodo | 228 | Attende una richiesta `GET /callback` e restituisce il codice che porta |
| `html_response` | fn | 271 | Avvolge una pagina HTML in una risposta 200 con content type HTML UTF-8 |
| `respond` | fn | 280 | Invia `response` al browser; un errore viene solo registrato |

## crates/app/src/main.rs

I tipi `MainWindow`, `Help`, `Page`, `StopType`, `TimeOption`, `TurnView`, `MeetingRow` sono generati da `ui/app-window.slint` nel modulo `ui` e non sono elencati.

| Nome | Tipo | Riga | Descrizione |
| --- | --- | --- | --- |
| `main` | fn | 28 | Avvia l'app: crea finestra e stato, collega la sessione e i callback della UI, poi esegue il ciclo eventi Slint |
| `play_for` | fn | 483 | Riproduce il suono di un TickSound: nulla per nessuno, suono di fine a fine riunione, beep morbido a fine turno |
| `connect_session` | fn | 496 | Crea la sessione verso il backend se l'app è compilata con un URL di backend |

## crates/app/src/pkce.rs

| Nome | Tipo | Riga | Descrizione |
| --- | --- | --- | --- |
| `PkceError` | enum | 9 | Errore nella generazione di byte casuali |
| `random_base64url` | fn | 16 | Restituisce `len` byte casuali codificati in base64url senza padding |
| `generate_verifier` | fn | 27 | Genera il code verifier PKCE: 32 byte casuali in base64url, 43 caratteri |
| `generate_state` | fn | 36 | Genera il parametro `state` del login: 24 byte casuali in base64url, 32 caratteri |
| `challenge_for` | fn | 43 | Calcola la code challenge S256 di un verifier: base64url senza padding del suo SHA-256 |

## crates/app/src/session.rs

| Nome | Tipo | Riga | Descrizione |
| --- | --- | --- | --- |
| `LOGIN_TIMEOUT` | const | 27 | Tempo massimo di attesa del ritorno dal browser durante il login: 5 minuti |
| `MEETINGS_FAILED_MESSAGE` | const | 30 | Messaggio mostrato nella pagina riunioni quando non si caricano per altri motivi |
| `AppEvent` | enum | 34 | Risultato di un worker, consegnato al thread della UI |
| `StartupOutcome` | enum | 57 | Esito del controllo all'avvio |
| `startup_outcome` | fn | 70 | Traduce il risultato del recupero profilo all'avvio in ciò che la UI deve mostrare |
| `meetings_error_message` | fn | 85 | Messaggio per l'utente su un recupero riunioni fallito |
| `remaining_day_range` | fn | 97 | Intervallo di riunioni da recuperare: da `now` alle 23:59:59.999 dello stesso giorno nel fuso di `now` |
| `EventHandler` | alias di tipo | 106 | Callback del thread UI che riceve gli eventi dei worker |
| `set_event_handler` | fn | 115 | Installa l'handler UI degli eventi dei worker, sostituendo quello precedente |
| `clear_event_handler` | fn | 120 | Rimuove l'handler, rilasciando ciò che catturava; gli eventi successivi sono ignorati |
| `post` | fn | 127 | Invia `event` da qualsiasi thread all'handler del thread UI |
| `ActiveLogin` | struct | 140 | Tentativo di login in attesa del browser |
| `Session` | struct | 147 | Handle sul thread UI della sessione backend; non è `Send` e resta sul thread UI |
| `Session::connect` | metodo | 164 | Apre il portachiavi del sistema operativo e crea il client backend per `base_url` |
| `Session::spawn_startup_check` | metodo | 184 | Controlla la sessione salvata su un worker e posta `AppEvent::StartupChecked` |
| `Session::start_login` | metodo | 203 | Avvia il login nel browser su un worker, che termina con `AppEvent::LoginFinished` |
| `Session::cancel_login` | metodo | 228 | Annulla il login in attesa, se presente; il suo risultato verrà ignorato |
| `Session::finish_login` | metodo | 236 | Segna come concluso il login `attempt`; restituisce `false` se non è quello in attesa |
| `Session::spawn_meetings_fetch` | metodo | 252 | Recupera su un worker le riunioni restanti di oggi e posta `AppEvent::MeetingsLoaded` |
| `Session::is_current_meetings_request` | metodo | 277 | Vero se `request` è l'ultimo recupero riunioni, quindi il suo risultato serve ancora |
| `Session::spawn_logout` | metodo | 285 | Revoca la sessione su un worker e posta `AppEvent::LoggedOut` |
| `apply_event` | fn | 302 | Applica un evento dei worker allo stato UI, ignorando i risultati obsoleti di login e riunioni |
| `check_saved_session` | fn | 341 | Esegue il controllo all'avvio: senza sessione chiede il login, altrimenti recupera il profilo |
| `run_login` | fn | 361 | Esegue un intero login nel browser: segreti PKCE, listener loopback, browser, attesa della callback |
| `open_system_browser` | fn | 386 | Apre `url` nel browser predefinito |
| `lock` | fn | 391 | Blocca il client condiviso; un lock avvelenato viene recuperato, mai un errore |
| `spawn` | fn | 400 | Avvia un thread worker con nome che esegue `job` |

## crates/app/src/session\_store.rs

| Nome | Tipo | Riga | Descrizione |
| --- | --- | --- | --- |
| `KEYRING_SERVICE` | const | 8 | Nome del servizio nel portachiavi sotto cui è salvato il refresh token |
| `KEYRING_USER` | const | 10 | Nome dell'account nel portachiavi della voce del refresh token |
| `SessionStoreError` | enum | 14 | Errore di uno store di sessione |
| `SessionStore` | trait | 25 | Archivio per il refresh token di lunga durata |
| `init_keyring` | fn | 58 | Imposta il portachiavi predefinito di processo usato da `KeyringStore` |
| `KeyringStore` | struct | 79 | Store di sessione basato sul portachiavi del sistema operativo; richiede prima `init_keyring` |
| `KeyringStore::entry` | metodo | 82 | Crea la voce del portachiavi per servizio e utente del refresh token |
| `SessionStore for KeyringStore` | trait impl | 87 | Implementa `SessionStore` leggendo, scrivendo ed eliminando il token nel portachiavi del sistema |
| `KeyringStore::load_refresh_token` | metodo | 90 | Legge il token; una voce mancante dà `Ok(None)`, senza store predefinito dà errore keyring |
| `KeyringStore::save_refresh_token` | metodo | 98 | Salva il refresh token nella voce del portachiavi |
| `KeyringStore::clear` | metodo | 104 | Elimina la voce del portachiavi; una voce mancante non è un errore |
| `MemoryStore` | struct | 115 | Store di sessione in memoria; i cloni condividono lo stesso slot, utile nei test |
| `MemoryStore::new` | metodo | 121 | Crea uno store vuoto |
| `MemoryStore::set` | metodo | 126 | Sostituisce il contenuto dello slot; un lock avvelenato viene recuperato |
| `SessionStore for MemoryStore` | trait impl | 132 | Implementa `SessionStore` tenendo il token in uno slot condiviso in memoria |
| `MemoryStore::load_refresh_token` | metodo | 133 | Restituisce una copia del token presente nello slot |
| `MemoryStore::save_refresh_token` | metodo | 138 | Salva il token nello slot |
| `MemoryStore::clear` | metodo | 143 | Svuota lo slot |

## crates/app/src/sound.rs

| Nome | Tipo | Riga | Descrizione |
| --- | --- | --- | --- |
| `SOUND_ASSET_NAME` | const | 12 | Nome del file audio `arcade_victory.mp3` riprodotto a fine timer |
| `BEEP_FREQUENCY_HZ` | const | 13 | Frequenza del beep di fine turno: 880 Hz |
| `BEEP_DURATION` | const | 14 | Durata del beep di fine turno: 200 ms |
| `BEEP_AMPLITUDE` | const | 15 | Volume del beep di fine turno: 0,2 |
| `sound_path` | fn | 23 | Individua il file audio cercando in tutte le posizioni possibili tra sviluppo e installazione |
| `play_sound` | fn | 55 | Riproduce una volta il suono di fine su un thread in background e ritorna subito |
| `play_beep` | fn | 95 | Riproduce su un thread in background un beep sintetizzato (onda sinusoidale): segna la fine di un turno di parola |

## crates/app/src/state.rs

| Nome | Tipo | Riga | Descrizione |
| --- | --- | --- | --- |
| `LOGIN_WAITING_STATUS` | const | 20 | Stato della pagina di login mentre il login nel browser è in corso |
| `OFFLINE_NOTICE` | const | 22 | Avviso della pagina manuale quando il backend non è raggiungibile all'avvio |
| `LOCAL_MODE_NOTICE` | const | 24 | Avviso della pagina manuale quando non si può creare il client backend |
| `LOGOUT_NOTICE` | const | 26 | Avviso della pagina manuale mentre è in corso il logout |
| `DEFAULT_TURN_MINUTES` | const | 29 | Minuti con cui parte un turno di parola: 5 |
| `MINUTE_MS` | const | 30 | Millisecondi in un minuto |
| `DAILY_FINISHED_NAME` | const | 32 | Nome mostrato dalla schermata Daily quando hanno parlato tutti ("Tutti hanno parlato") |
| `TickSound` | enum | 36 | Suono che il chiamante deve riprodurre dopo un aggiornamento: nessuno, fine riunione o fine turno (beep morbido) |
| `MeetingsStatus` | enum | 47 | Cosa mostra la pagina riunioni oltre alle righe |
| `TimerOrigin` | enum | 58 | Pagina da cui è partito il conto alla rovescia (manuale o riunioni), per tornarci uscendone |
| `TIMER_WINDOW_SIZE` | const | 66 | Dimensione della finestra mentre si mostra un conto alla rovescia: 360x240 |
| `SavedWindow` | struct | 70 | Dimensione e stato "sempre in primo piano" della finestra da ripristinare uscendo dal conto alla rovescia |
| `WindowStep` | enum | 77 | Cosa deve fare `render` alla finestra: mantenerla, ridurla (Enter) o ripristinarla (Leave) |
| `AppState` | struct | 87 | Tutto ciò che mostra la finestra principale, posseduto da Rust e inviato a Slint da `AppState::render` |
| `AppState::new` | metodo | 145 | Crea lo stato sulla pagina manuale, con opzioni calcolate dall'ora corrente |
| `AppState::refresh_options` | metodo | 186 | Ricalcola gli orari di fine selezionabili dall'ora corrente; lo slot scelto resta se ancora offerto, altrimenti si azzera; ricostruisce anche le righe riunione |
| `AppState::select_slot` | metodo | 209 | Sceglie lo slot di fine `index` nella pagina manuale senza avviare il conto alla rovescia; indice non valido ignorato |
| `AppState::start_selected` | metodo | 230 | Avvia il conto alla rovescia verso lo slot scelto e passa alla pagina timer; senza scelta o con fine già passata non parte |
| `AppState::select_meeting` | metodo | 266 | Avvia il conto alla rovescia verso la fine della riunione della riga `index` (anche non ancora iniziata); con Daily attivo e partecipanti parte la schermata Daily |
| `AppState::tick` | metodo | 315 | Aggiorna il conto alla rovescia e i turni rispetto all'ora corrente e restituisce il suono da riprodurre (fine riunione o fine turno) |
| `AppState::stop` | metodo | 344 | Termina il conto alla rovescia: alla pagina di fine se `TimerFinished`, altrimenti alla pagina da cui era partito |
| `AppState::turns_available` | metodo | 362 | Vero se il conto alla rovescia è di una riunione con almeno un partecipante, quindi i turni di parola sono usabili |
| `AppState::show_turns` | metodo | 371 | Apre le impostazioni dei turni di parola dalla vista semplice |
| `AppState::hide_turns` | metodo | 384 | Nasconde le impostazioni dei turni e torna alla vista semplice; il conto continua |
| `AppState::select_turner` | metodo | 393 | Sceglie il partecipante del prossimo turno; indice non valido ignorato |
| `AppState::adjust_turn_minutes` | metodo | 406 | Sposta i minuti del prossimo turno di `delta`, tra 1 e il massimo che entra nella riunione |
| `AppState::start_turn` | metodo | 421 | Avvia il turno del partecipante e dei minuti scelti, limitato alla fine della riunione |
| `AppState::adjust_turn_time` | metodo | 444 | Aggiunge o toglie minuti al turno in corso, senza superare la fine della riunione né scendere sotto zero |
| `AppState::end_turn` | metodo | 456 | Termina il turno in corso e torna alle impostazioni dei turni |
| `AppState::set_daily` | metodo | 468 | Imposta l'interruttore Daily della riunione della riga `index` e lo ricorda per quell'evento finché l'app è aperta |
| `AppState::daily_on` | metodo | 489 | Vero se l'interruttore Daily è attivo: la scelta dell'utente, altrimenti se il titolo sembra una daily |
| `AppState::toggle_daily_pause` | metodo | 499 | Mette in pausa il relatore Daily o lo riprende |
| `AppState::next_daily_speaker` | metodo | 509 | Passa la parola al prossimo relatore Daily |
| `AppState::adjust_daily_time` | metodo | 520 | Aggiunge o toglie minuti al relatore Daily, nei limiti del giro del core |
| `AppState::reset_turns` | metodo | 529 | Dimentica le impostazioni dei turni e installa i partecipanti di un nuovo conto alla rovescia: vista semplice, primo scelto, minuti di default |
| `AppState::turn_clock` | metodo | 541 | Ora corrente e fine del conto alla rovescia in millisecondi; `None` senza conto in corso |
| `AppState::fit_turn_minutes` | metodo | 547 | Abbassa i minuti del prossimo turno a quanto la riunione consente ancora |
| `AppState::tick_turns` | metodo | 555 | Parte di un tick relativa ai turni: tiene validi i minuti delle impostazioni o controlla se il turno (o il relatore Daily) è finito |
| `AppState::show_manual_page` | metodo | 586 | Passa alla pagina manuale senza toccare conto alla rovescia né opzioni |
| `AppState::show_meetings_page` | metodo | 592 | Passa alla pagina riunioni mantenendo quelle già caricate; senza account va alla pagina manuale |
| `AppState::show_home` | metodo | 602 | Lascia le schermate del conto alla rovescia per la pagina da cui era partito: manuale o riunioni; l'origine viene dimenticata |
| `AppState::on_meetings_loaded` | metodo | 610 | Sostituisce le riunioni con quelle di un recupero riuscito, scarta gli interruttori Daily di eventi spariti e mostra le righe |
| `AppState::on_meetings_failed` | metodo | 621 | Mostra `message` nella pagina riunioni, come avviso sopra la lista se ci sono già riunioni |
| `AppState::on_session_lost` | metodo | 627 | Sessione persa: ferma il conto alla rovescia, azzera i turni e passa alla pagina di login con `status` |
| `AppState::forget_meetings` | metodo | 636 | Scarta le riunioni caricate, così la prossima visita alla pagina riunioni ricomincia il caricamento |
| `AppState::render_turns` | metodo | 645 | Invia alla finestra la vista dei turni, le impostazioni e il turno in corso, poi la schermata Daily |
| `AppState::render_daily` | metodo | 668 | Invia alla finestra la schermata Daily; senza giro tutto è vuoto; aggiorna i chip solo se cambiati |
| `AppState::sync_window` | metodo | 706 | Riduce la finestra alla forma del conto alla rovescia all'apertura e la ripristina all'uscita, salvando dimensione e stato in primo piano; a schermo intero non cambia nulla |
| `AppState::selected_index` | metodo | 737 | Posizione dello slot scelto tra le opzioni, o -1 se nessuno |
| `AppState::is_signed_in` | metodo | 748 | Vero se è autenticato un account |
| `AppState::home_page` | metodo | 754 | Pagina a cui torna un conto alla rovescia finito o fermato: riunioni se autenticato, altrimenti manuale |
| `AppState::rebuild_meeting_rows` | metodo | 766 | Ricostruisce riunioni mostrate e righe da quelle ricevute, dall'ora corrente e dagli interruttori Daily |
| `AppState::show_loading` | metodo | 779 | Passa alla pagina di caricamento mostrata mentre si controlla la sessione salvata |
| `AppState::on_signed_in` | metodo | 784 | Registra l'account autenticato e passa alla pagina riunioni, in attesa di un recupero |
| `AppState::on_login_required` | metodo | 794 | Passa alla pagina di login inattiva con `status` (vuoto per nessuno), dimenticando l'account |
| `AppState::on_local_mode` | metodo | 803 | Passa alla pagina manuale senza account, mostrando `notice` (vuoto per nessuno) |
| `AppState::on_login_started` | metodo | 811 | Mostra la pagina di login in attesa del browser |
| `AppState::on_login_failed` | metodo | 817 | Termina l'attesa nella pagina di login e mostra `message` |
| `AppState::on_login_cancelled` | metodo | 823 | Termina l'attesa nella pagina di login senza messaggio |
| `AppState::on_logout_started` | metodo | 830 | Nasconde account e riunioni durante il logout e segnala che è in corso |
| `AppState::on_logged_out` | metodo | 838 | Ferma il conto alla rovescia, dimentica l'account e passa alla pagina di login inattiva |
| `AppState::bind_options` | metodo | 850 | Collega il modello condiviso delle opzioni a `window`; da chiamare una volta dopo aver creato la finestra |
| `AppState::bind_meetings` | metodo | 855 | Collega il modello condiviso delle righe riunione a `window`; da chiamare una volta dopo la creazione |
| `AppState::bind_turns` | metodo | 860 | Collega alla finestra i modelli dei partecipanti e dei chip Daily (una volta) |
| `AppState::render` | metodo | 867 | Invia a `window` pagina, forma della finestra, timer, turni, Daily, account e righe di stato |
| `account_label` | fn | 924 | Nome mostrato per l'account: il nome visualizzato, o l'email se manca o è vuoto |
| `is_in_progress` | fn | 932 | Vero se la riunione è iniziata e non ancora finita a now |
| `visible_meetings` | fn | 941 | Riunioni da mostrare a now: non finite, quella in corso in cima, le altre per inizio |
| `clamp_turn_minutes` | fn | 954 | Limita i minuti di un turno al massimo che entra nella riunione, con minimo 1 |
| `shows_countdown` | fn | 959 | Vero per le pagine che mostrano un conto alla rovescia: quello in corso e quello finito |
| `window_step` | fn | 965 | Decide come cambia la forma della finestra passando da una pagina senza conto alla rovescia a una con (o viceversa) |
| `resize_later` | fn | 974 | Accoda sul ciclo eventi il ridimensionamento della finestra |
| `random_seed` | fn | 987 | Numero casuale non negativo per la prima didascalia della pagina di fine; 0 se la sorgente casuale fallisce |
| `random_u64` | fn | 997 | Seme casuale per l'ordine di parola della Daily; 0 se la sorgente casuale fallisce |
| `DailyView` | struct | 1007 | Cosa mostra la schermata Daily per un giro a un dato istante |
| `daily_view` | fn | 1027 | Costruisce la `DailyView` di un giro: relatore, tempo, pausa, chip dei prossimi e riga riepilogo; a giro finito tutto vuoto e disabilitato |
| `format_minutes_seconds` | fn | 1060 | Formatta i millisecondi come `MM:SS` arrotondati al secondo; i minuti possono superare 59 |
| `meeting_row` | fn | 1068 | Costruisce la riga di una riunione: orari HH:MM–HH:MM, titolo, riga meta e interruttore Daily (scelta dell'utente o titolo da daily) |
| `meeting_meta` | fn | 1093 | Riga meta di una riunione: "In corso" o il tempo all'inizio, poi il numero di partecipanti accettati |
| `starts_in_text` | fn | 1112 | Testo per un inizio tra `minutes` minuti: "Tra 52 min", "Tra 1 h", "Tra 1 h 20 min" |
| `retain_selection` | fn | 1126 | Tiene lo slot scelto solo finché le opzioni offrono ancora quell'istante di fine |
| `time_range` | fn | 1139 | Inizio e fine del meeting come `HH:MM–HH:MM` nel fuso dato |
| `split_options` | fn | 1152 | Divide le opzioni di quarto d'ora in orari di fine e righe mostrate dalla UI |
