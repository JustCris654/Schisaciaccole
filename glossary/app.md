# Glossario — app

Crate `schisaciaccole` (`crates/app`): app desktop Slint. Aggiornato sul codice basato sul commit `3fd2613` del branch `v2`.

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
| `CONNECT_TIMEOUT` | const | 23 | Timeout di connessione di ogni richiesta: 5 secondi |
| `REQUEST_TIMEOUT` | const | 25 | Timeout totale di ogni richiesta: 15 secondi |
| `REFRESH_MARGIN` | const | 27 | Un access token in scadenza entro 30 secondi viene rinnovato prima dell'uso |
| `BackendError` | enum | 31 | Errore di una chiamata al backend |
| `AccessToken` | struct | 66 | Access token tenuto in memoria insieme alla scadenza |
| `BackendClient` | struct | 72 | Client dell'API backend, non thread-safe: da usare da un solo thread alla volta |
| `BackendClient::new` | metodo | 86 | Crea un client per il backend a `base_url`, ignorando gli slash finali |
| `BackendClient::login_url` | metodo | 109 | Costruisce l'URL da aprire nel browser per avviare il login |
| `BackendClient::has_session` | metodo | 120 | Vero se è salvato un refresh token; un errore dello store vale come nessuna sessione |
| `BackendClient::health` | metodo | 131 | Verifica che il backend sia attivo (`GET /healthz`) |
| `BackendClient::exchange_code` | metodo | 146 | Scambia il codice di login con i token e salva il refresh token |
| `BackendClient::fetch_me` | metodo | 168 | Recupera l'utente autenticato |
| `BackendClient::fetch_meetings` | metodo | 180 | Recupera le riunioni tra `from` e `to` |
| `BackendClient::fetch_plan` | metodo | 201 | `GET /api/v1/meetings/{event_id}/plan`: piano slot del meeting; id escapato come segmento di path |
| `BackendClient::save_template` | metodo | 215 | `PUT /api/v1/templates`: crea o sostituisce la preferenza del titolo e restituisce il `Template` |
| `BackendClient::delete_template` | metodo | 232 | `DELETE /api/v1/templates/{id}`: cancella una preferenza; la risposta 204 non viene decodificata |
| `BackendClient::logout` | metodo | 246 | Revoca la sessione sul backend e la dimentica in locale |
| `BackendClient::url` | metodo | 258 | Concatena l'URL di base del backend con un percorso |
| `BackendClient::post_json` | metodo | 262 | Invia una richiesta POST con corpo JSON al percorso indicato |
| `BackendClient::accept_tokens` | metodo | 267 | Tiene l'access token in memoria e salva il refresh token, eventualmente ruotato |
| `BackendClient::clear_session` | metodo | 278 | Dimentica la sessione in memoria e nello store; un errore dello store viene solo registrato |
| `BackendClient::ensure_access` | metodo | 286 | Restituisce un access token valido, rinnovandolo se manca o sta per scadere |
| `BackendClient::refresh` | metodo | 297 | Rinnova l'access token con il refresh token salvato, che ruota a ogni uso |
| `BackendClient::refresh_failure` | metodo | 314 | Interpreta un rinnovo fallito: sessione morta o ri-autenticazione Google svuotano lo store |
| `BackendClient::api_failure` | metodo | 323 | Interpreta una chiamata API fallita: una richiesta di ri-autenticazione Google svuota lo store |
| `BackendClient::segments_url` | metodo | 335 | URL della base più i segmenti dati, ognuno escapato; mantiene il prefisso di path della base |
| `BackendClient::request_once` | metodo | 344 | Un tentativo di richiesta autenticata con metodo, query e corpo JSON opzionali |
| `BackendClient::send_authenticated` | metodo | 365 | Richiesta autenticata: rinnova il token se serve, ritenta una volta su 401 `unauthorized`, mappa gli errori |
| `BackendClient::json_request` | metodo | 387 | Richiesta autenticata con risposta JSON decodificata |
| `BackendClient::get_json` | metodo | 399 | GET autenticata con risposta JSON, tramite `json_request` |
| `has_code` | fn | 410 | Vero se `error` è un errore del server con il codice API indicato |
| `send` | fn | 415 | Invia una richiesta trasformando errori di connessione e stati non riusciti in errori |
| `error_from` | fn | 425 | Costruisce l'errore di una risposta non riuscita; il 429 diventa `RateLimited` |
| `decode` | fn | 441 | Decodifica il corpo JSON della risposta, mappando il fallimento su `BackendError::Decode` |

## crates/app/src/config.rs

| Nome | Tipo | Riga | Descrizione |
| --- | --- | --- | --- |
| `backend_url` | fn | 7 | URL base del backend fissato in compilazione da `SCHISACIACCOLE_BACKEND_URL` |
| `normalize_backend_url` | fn | 15 | Toglie spazi e slash finali da un URL del backend |

## crates/app/src/editor.rs

| Nome | Tipo | Riga | Descrizione |
| --- | --- | --- | --- |
| `MAX_ROWS` | const | 17 | Numero massimo di righe dell'editor (99) |
| `MAX_MINUTES` | const | 19 | Minuti massimi di una riga; valori più alti vengono limitati (600) |
| `NEW_ROW_LABEL` | const | 21 | Etichetta di una riga aggiunta a mano ("Voce") |
| `NEW_ROW_MINUTES` | const | 23 | Minuti di una riga aggiunta a mano (5) |
| `SlotEditor` | struct | 27 | Righe in minuti dell'editor slot di un meeting, con durata nominale e id della preferenza |
| `SlotEditor::new` | metodo | 40 | Editor vuoto per un meeting; durata nominale fine meno inizio, mai negativa |
| `SlotEditor::meeting` | metodo | 55 | Meeting in modifica |
| `SlotEditor::event_id` | metodo | 60 | Id evento del meeting in modifica |
| `SlotEditor::rows` | metodo | 65 | Righe correnti dell'editor |
| `SlotEditor::template_id` | metodo | 70 | Id della preferenza salvata, se nota |
| `SlotEditor::set_template_id` | metodo | 75 | Sostituisce l'id della preferenza nota |
| `SlotEditor::duration_minutes` | metodo | 80 | Durata nominale in minuti interi (per difetto) |
| `SlotEditor::load_plan` | metodo | 86 | Sostituisce le righe con quelle di un piano sulla durata nominale e ne ricorda l'id della preferenza |
| `SlotEditor::reset_to_equal` | metodo | 95 | Sostituisce le righe con il piano a slot uguali dei partecipanti; l'id della preferenza resta |
| `SlotEditor::set_label` | metodo | 103 | Cambia l'etichetta di una riga; indice fuori range ignorato |
| `SlotEditor::set_minutes` | metodo | 113 | Cambia i minuti di una riga, limitati a 0..=600 |
| `SlotEditor::set_included` | metodo | 125 | Include o esclude una riga partecipante; le righe manuali restano invariate |
| `SlotEditor::move_up` | metodo | 136 | Sposta una riga in su; la prima resta dov'è |
| `SlotEditor::move_down` | metodo | 147 | Sposta una riga in giù; l'ultima resta dov'è |
| `SlotEditor::add_manual_row` | metodo | 158 | Aggiunge in fondo una riga manuale "Voce" da 5 minuti, fino a `MAX_ROWS` |
| `SlotEditor::remove_row` | metodo | 176 | Toglie una riga manuale; le righe partecipante non si tolgono |
| `SlotEditor::unassigned_minutes` | metodo | 188 | Minuti non assegnati sulla durata nominale; negativo se oltre la durata |
| `SlotEditor::is_over` | metodo | 193 | Vero se le righe incluse superano la durata del meeting |
| `SlotEditor::unassigned_text` | metodo | 199 | Testo del piè di pagina: "Non assegnati: N min" o "Oltre la durata di N min" |
| `SlotEditor::save_request` | metodo | 212 | Richiesta di salvataggio della preferenza dalle righe (esclusi a peso 0, tempo libero in fondo) |
| `SlotEditor::start_slots` | metodo | 224 | Slot `(etichetta, peso)` per avviare il timer dalle righe, senza voci a peso 0 |

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

I tipi `MainWindow`, `Help`, `Page`, `StopType`, `TimeOption`, `MeetingRow` sono generati da `ui/app-window.slint` nel modulo `ui` e non sono elencati.

| Nome | Tipo | Riga | Descrizione |
| --- | --- | --- | --- |
| `main` | fn | 30 | Avvia l'app: crea finestra e stato, collega la sessione e i callback della UI, poi esegue il ciclo eventi Slint |
| `update_state` | fn | 465 | Esegue un'azione sullo stato e aggiorna la finestra, se esiste ancora |
| `play_for` | fn | 480 | Riproduce il suono di un TickSound: beep a fine slot, suono di fine a fine riunione |
| `connect_session` | fn | 493 | Crea la sessione verso il backend se l'app è compilata con un URL di backend |

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
| `AppEvent` | enum | 34 | Risultato di un worker, consegnato al thread della UI (anche `PlanLoaded` con scopo, `TemplateSaved`, `TemplateDeleted`) |
| `PlanPurpose` | enum | 87 | Scopo di una richiesta di piano (`Timer` o `Editor`), con id di richiesta separati |
| `StartupOutcome` | enum | 96 | Esito del controllo all'avvio |
| `startup_outcome` | fn | 109 | Traduce il risultato del recupero profilo all'avvio in ciò che la UI deve mostrare |
| `meetings_error_message` | fn | 124 | Messaggio per l'utente su un recupero riunioni fallito |
| `remaining_day_range` | fn | 136 | Intervallo di riunioni da recuperare: da `now` alle 23:59:59.999 dello stesso giorno nel fuso di `now` |
| `EventHandler` | alias di tipo | 145 | Callback del thread UI che riceve gli eventi dei worker |
| `set_event_handler` | fn | 154 | Installa l'handler UI degli eventi dei worker, sostituendo quello precedente |
| `clear_event_handler` | fn | 159 | Rimuove l'handler, rilasciando ciò che catturava; gli eventi successivi sono ignorati |
| `post` | fn | 166 | Invia `event` da qualsiasi thread all'handler del thread UI |
| `ActiveLogin` | struct | 179 | Tentativo di login in attesa del browser |
| `Session` | struct | 186 | Handle sul thread UI della sessione backend; non è `Send` e resta sul thread UI |
| `Session::connect` | metodo | 209 | Apre il portachiavi del sistema operativo e crea il client backend per `base_url` |
| `Session::spawn_startup_check` | metodo | 232 | Controlla la sessione salvata su un worker e posta `AppEvent::StartupChecked` |
| `Session::start_login` | metodo | 251 | Avvia il login nel browser su un worker, che termina con `AppEvent::LoginFinished` |
| `Session::cancel_login` | metodo | 276 | Annulla il login in attesa, se presente; il suo risultato verrà ignorato |
| `Session::finish_login` | metodo | 284 | Segna come concluso il login `attempt`; restituisce `false` se non è quello in attesa |
| `Session::spawn_meetings_fetch` | metodo | 300 | Recupera su un worker le riunioni restanti di oggi e posta `AppEvent::MeetingsLoaded` |
| `Session::is_current_meetings_request` | metodo | 325 | Vero se `request` è l'ultimo recupero riunioni, quindi il suo risultato serve ancora |
| `Session::spawn_plan_fetch` | metodo | 335 | Chiede il piano slot di un meeting su un thread di lavoro e pubblica `PlanLoaded` con lo scopo |
| `Session::is_current_plan_request` | metodo | 364 | Vero se la richiesta di piano è l'ultima per quello scopo |
| `Session::plan_counter` | metodo | 369 | Contatore degli id delle richieste di piano per uno scopo |
| `Session::spawn_template_save` | metodo | 381 | Salva una preferenza su un thread di lavoro e pubblica `TemplateSaved` |
| `Session::spawn_template_delete` | metodo | 412 | Cancella una preferenza su un thread di lavoro e pubblica `TemplateDeleted` |
| `Session::next_template_request` | metodo | 439 | Prende l'id di una nuova richiesta di salvataggio o cancellazione |
| `Session::is_current_template_request` | metodo | 447 | Vero se la richiesta di salvataggio o cancellazione è l'ultima |
| `Session::spawn_logout` | metodo | 455 | Revoca la sessione su un worker e posta `AppEvent::LoggedOut` |
| `apply_event` | fn | 477 | Applica un evento dei worker allo stato UI, ignorando i risultati obsoleti; smista i piani a timer o editor e ricarica la lista dopo salvataggio o cancellazione |
| `check_saved_session` | fn | 557 | Esegue il controllo all'avvio: senza sessione chiede il login, altrimenti recupera il profilo |
| `run_login` | fn | 577 | Esegue un intero login nel browser: segreti PKCE, listener loopback, browser, attesa della callback |
| `open_system_browser` | fn | 602 | Apre `url` nel browser predefinito |
| `lock` | fn | 607 | Blocca il client condiviso; un lock avvelenato viene recuperato, mai un errore |
| `spawn` | fn | 616 | Avvia un thread worker con nome che esegue `job` |

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
| `BEEP_FREQUENCY_HZ` | const | 13 | Frequenza del beep di fine slot: 880 Hz |
| `BEEP_DURATION` | const | 14 | Durata del beep di fine slot: 200 ms |
| `BEEP_AMPLITUDE` | const | 15 | Volume del beep di fine slot: 0,2 |
| `sound_path` | fn | 23 | Individua il file audio cercando in tutte le posizioni possibili tra sviluppo e installazione |
| `play_sound` | fn | 55 | Riproduce una volta il suono di fine su un thread in background e ritorna subito |
| `play_beep` | fn | 95 | Riproduce su un thread in background un beep sintetizzato (onda sinusoidale) che segna la fine di uno slot |

## crates/app/src/state.rs

| Nome | Tipo | Riga | Descrizione |
| --- | --- | --- | --- |
| `LOGIN_WAITING_STATUS` | const | 23 | Stato della pagina di login mentre il login nel browser è in corso |
| `OFFLINE_NOTICE` | const | 25 | Avviso della pagina di selezione quando il backend non è raggiungibile all'avvio |
| `LOCAL_MODE_NOTICE` | const | 27 | Avviso della pagina di selezione quando non si può creare il client backend |
| `LOGOUT_NOTICE` | const | 29 | Avviso della pagina di selezione mentre è in corso il logout |
| `EDITOR_PLAN_FALLBACK_NOTICE` | const | 32 | Avviso dell'editor quando il piano non arriva: "Preferenza non caricata: slot uguali" |
| `TEMPLATE_SAVING_NOTICE` | const | 34 | Avviso dell'editor durante il salvataggio |
| `TEMPLATE_SAVED_NOTICE` | const | 36 | Avviso dell'editor dopo un salvataggio riuscito |
| `TEMPLATE_INVALID_NOTICE` | const | 38 | Avviso dell'editor quando il backend rifiuta la preferenza (400) |
| `TEMPLATE_SAVE_FAILED_NOTICE` | const | 40 | Avviso dell'editor per un salvataggio non riuscito |
| `TEMPLATE_DELETING_NOTICE` | const | 42 | Avviso dell'editor durante la cancellazione |
| `TEMPLATE_DELETED_NOTICE` | const | 44 | Avviso dell'editor dopo una cancellazione riuscita |
| `TEMPLATE_DELETE_FAILED_NOTICE` | const | 46 | Avviso dell'editor per una cancellazione non riuscita |
| `MEETING_OVER_NOTICE` | const | 48 | Avviso dell'editor quando si avvia un meeting già terminato |
| `TickSound` | enum | 52 | Suono che il chiamante deve riprodurre dopo un aggiornamento: nessuno, fine slot (beep) o fine riunione |
| `MeetingSelection` | struct | 63 | Esito di `select_meeting`: suono da riprodurre ed evento di cui chiedere il piano |
| `MeetingRun` | struct | 72 | Meeting del countdown in corso: evento, istante del clic e piano in attesa |
| `TemplateSave` | struct | 83 | Salvataggio di preferenza che il chiamante deve inviare |
| `TemplateDelete` | struct | 92 | Cancellazione di preferenza che il chiamante deve inviare |
| `OpenEditor` | struct | 100 | Editor aperto su un meeting, con stato di caricamento, richiesta in corso e avviso |
| `MeetingsStatus` | enum | 112 | Cosa mostra la pagina riunioni oltre alle righe |
| `AppState` | struct | 122 | Tutto ciò che mostra la finestra principale, posseduto da Rust e inviato a Slint da `AppState::render` |
| `AppState::new` | metodo | 167 | Crea lo stato sulla pagina di selezione, con opzioni calcolate dall'ora corrente |
| `AppState::refresh_options` | metodo | 200 | Ricalcola gli orari di fine selezionabili dall'ora corrente |
| `AppState::select` | metodo | 226 | Avvia il conto alla rovescia verso l'opzione `index` e passa alla pagina timer |
| `AppState::select_meeting` | metodo | 262 | Avvia subito il conto alla rovescia verso la fine della riunione della riga `index`, senza slot, e restituisce l'evento di cui chiedere il piano |
| `AppState::start_run` | metodo | 298 | Avvia il countdown verso la fine del meeting dall'istante del clic e passa alla pagina timer |
| `AppState::install_slots` | metodo | 324 | Divide il tempo da `started_at` a `target` negli slot pesati e imposta lo slot corrente senza suono |
| `AppState::on_plan_loaded` | metodo | 346 | Applica il piano arrivato per il timer; su errore usa slot uguali con avviso, senza fermare il timer |
| `AppState::apply_plan` | metodo | 361 | Come `on_plan_loaded` con l'istante esplicito; scarta piani obsoleti |
| `AppState::open_editor` | metodo | 398 | Apre l'editor slot sul meeting della riga e chiede il piano (scopo editor) |
| `AppState::close_editor` | metodo | 422 | Chiude l'editor e torna alla lista meeting |
| `AppState::on_editor_plan_loaded` | metodo | 436 | Carica nell'editor il piano arrivato; su errore slot uguali con avviso |
| `AppState::editor_set_label` | metodo | 464 | Cambia l'etichetta di una riga dell'editor |
| `AppState::editor_set_minutes` | metodo | 473 | Cambia i minuti di una riga dell'editor |
| `AppState::editor_set_included` | metodo | 483 | Include o esclude una riga partecipante dell'editor |
| `AppState::editor_move_up` | metodo | 492 | Sposta in su una riga dell'editor |
| `AppState::editor_move_down` | metodo | 501 | Sposta in giù una riga dell'editor |
| `AppState::editor_remove_row` | metodo | 510 | Toglie una riga manuale dell'editor |
| `AppState::editor_add_row` | metodo | 519 | Aggiunge una riga manuale all'editor |
| `AppState::editor_reset` | metodo | 529 | Riporta l'editor a slot uguali |
| `AppState::start_from_editor` | metodo | 546 | Avvia il countdown con gli slot dell'editor; meeting già finito: resta sull'editor con avviso |
| `AppState::start_template_save` | metodo | 582 | Prepara il salvataggio della preferenza e segna l'editor occupato |
| `AppState::on_template_saved` | metodo | 602 | Applica l'esito del salvataggio; true se va ricaricata la lista meeting |
| `AppState::start_template_delete` | metodo | 632 | Prepara la cancellazione della preferenza nota e segna l'editor occupato |
| `AppState::on_template_deleted` | metodo | 653 | Applica l'esito della cancellazione (anche 404 not\_found); true se va ricaricata la lista meeting |
| `AppState::on_template_failed` | metodo | 683 | Gestisce un errore di salvataggio o cancellazione: sessione persa, 400 o errore generico |
| `AppState::editor_on` | metodo | 706 | Editor aperto sull'evento dato, se c'è |
| `AppState::busy_editor` | metodo | 713 | Editor aperto sull'evento dato con una richiesta in corso, se c'è |
| `AppState::idle_editor` | metodo | 718 | Editor caricato e senza richieste in corso, che può inviarne una |
| `AppState::editable` | metodo | 725 | Editor aperto e caricato, le cui righe si possono modificare |
| `AppState::tick` | metodo | 748 | Aggiorna il conto alla rovescia rispetto all'ora corrente |
| `AppState::update_slot_index` | metodo | 773 | Salva l'indice dello slot corrente e segnala con SlotEnd se è avanzato da solo dall'ultimo tick |
| `AppState::next_slot` | metodo | 800 | Chiude lo slot e ridistribuisce il tempo sui successivi; sull'ultimo avvia Tempo libero senza cambiare il timer; nessun effetto senza slot, fuori dal timer o già in Tempo libero |
| `AppState::clear_slots` | metodo | 820 | Scarta slot, indice visto, avviso partecipanti e piano in attesa |
| `AppState::stop` | metodo | 834 | Termina il conto alla rovescia: aggiorna le opzioni, azzera target e timer |
| `AppState::show_selection_page` | metodo | 848 | Passa alla pagina di selezione senza toccare conto alla rovescia né opzioni |
| `AppState::show_meetings_page` | metodo | 854 | Passa alla pagina riunioni mantenendo quelle già caricate; senza account va altrove |
| `AppState::show_home` | metodo | 859 | Lascia la pagina di fine: riunioni se autenticato, altrimenti selezione |
| `AppState::on_meetings_loaded` | metodo | 864 | Sostituisce le riunioni con quelle di un recupero riuscito e le mostra |
| `AppState::on_meetings_failed` | metodo | 873 | Mostra `message` nella pagina riunioni, come avviso sopra la lista se ci sono già riunioni |
| `AppState::on_session_lost` | metodo | 879 | Sessione persa: ferma il conto alla rovescia e passa alla pagina di login con `status` |
| `AppState::forget_meetings` | metodo | 887 | Scarta le riunioni caricate, così la prossima visita alla pagina riunioni ricomincia il caricamento |
| `AppState::is_signed_in` | metodo | 896 | Vero se è autenticato un account |
| `AppState::home_page` | metodo | 902 | Pagina a cui torna un conto alla rovescia finito o fermato: riunioni se autenticato |
| `AppState::rebuild_meeting_rows` | metodo | 914 | Ricostruisce riunioni mostrate e righe da quelle ricevute e dall'ora corrente |
| `AppState::show_loading` | metodo | 926 | Passa alla pagina di caricamento mostrata mentre si controlla la sessione salvata |
| `AppState::on_signed_in` | metodo | 932 | Registra l'account autenticato e passa alla pagina riunioni, in attesa di un recupero |
| `AppState::on_login_required` | metodo | 944 | Passa alla pagina di login inattiva con `status` (vuoto per nessuno), dimenticando l'account |
| `AppState::on_local_mode` | metodo | 955 | Passa alla pagina di selezione senza account, mostrando `notice` (vuoto per nessuno) |
| `AppState::on_login_started` | metodo | 964 | Mostra la pagina di login in attesa del browser |
| `AppState::on_login_failed` | metodo | 970 | Termina l'attesa nella pagina di login e mostra `message` |
| `AppState::on_login_cancelled` | metodo | 976 | Termina l'attesa nella pagina di login senza messaggio |
| `AppState::on_logout_started` | metodo | 983 | Nasconde account e riunioni durante il logout e segnala che è in corso |
| `AppState::on_logged_out` | metodo | 992 | Ferma il conto alla rovescia, dimentica l'account e passa alla pagina di login inattiva |
| `AppState::bind_options` | metodo | 1003 | Collega il modello condiviso delle opzioni a `window`; da chiamare una volta dopo aver creato la finestra |
| `AppState::bind_meetings` | metodo | 1008 | Collega il modello condiviso delle righe riunione a `window`; da chiamare una volta dopo la creazione |
| `AppState::bind_editor_rows` | metodo | 1013 | Collega il modello delle righe dell'editor alla finestra (una volta) |
| `AppState::render` | metodo | 1018 | Invia a `window` pagina, stato di esecuzione, timer, account e righe di stato |
| `AppState::render_editor` | metodo | 1084 | Invia alla finestra intestazione, righe, piè di pagina e stato dell'editor |
| `account_label` | fn | 1211 | Nome mostrato per l'account: il nome visualizzato, o l'email se manca o è vuoto |
| `is_in_progress` | fn | 1219 | Vero se la riunione è iniziata e non ancora finita a now |
| `visible_meetings` | fn | 1228 | Riunioni da mostrare a now: non finite, quella in corso in cima, le altre per inizio |
| `row_index` | fn | 1240 | Converte un indice di riga dalla UI; negativo diventa `None` (con log) |
| `is_not_found` | fn | 1251 | Vero se l'errore è un 404 `not_found` |
| `editor_row` | fn | 1260 | Costruisce la riga UI (`EditorRow`) di una voce dell'editor |
| `sync_editor_rows` | fn | 1272 | Aggiorna il modello delle righe; con lo stesso numero di righe sostituisce solo quelle cambiate, così il campo in modifica tiene il focus |
| `weighted_slots` | fn | 1287 | Voci di piano con peso > 0 come slot `(etichetta, peso)`, in ordine |
| `meeting_remaining_fraction` | fn | 1301 | Frazione restante sulla durata nominale del meeting, limitata a 0–1; durate non valide e timer scaduti danno zero |
| `meeting_row` | fn | 1317 | Costruisce la riga di una riunione: orari HH:MM–HH:MM, titolo, numero di partecipanti e segno di preferenza salvata |
| `time_range` | fn | 1338 | Inizio e fine del meeting come `HH:MM–HH:MM` nel fuso dato |
| `split_options` | fn | 1351 | Divide le opzioni di quarto d'ora in orari di fine e righe mostrate dalla UI |
