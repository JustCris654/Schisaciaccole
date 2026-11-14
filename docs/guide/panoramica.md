# Schisaciaccole — Documentazione

## Panoramica

Schisaciaccole è un countdown desktop per le cerimonie Scrum a tempo fisso. Invece di chiedersi quanto manca, si sceglie l'orario in cui il meeting deve finire e il timer conta alla rovescia fino a quel momento. Durante il meeting può gestire i turni di parola e la modalità Daily, con un tempo per ogni partecipante.

### Le due modalità

| Modalità | Quando | Cosa offre |
| --- | --- | --- |
| Locale | App compilata senza `SCHISACIACCOLE_BACKEND_URL`, server irraggiungibile, oppure "Usa senza accesso" | Solo la pagina Manuale: si sceglie l'orario di fine e parte il countdown. Nessuna chiamata di rete. |
| Con backend | App compilata con `SCHISACIACCOLE_BACKEND_URL` e utente autenticato con Google | I meeting di oggi letti da Google Calendar, i partecipanti (con l'espansione dei gruppi Google), la modalità Daily e le preferenze dei meeting (rinomine degli invitati e persone aggiunte a mano) salvate sul server. |

L'URL del backend è fissato al momento della compilazione. Il server tiene i token Google dell'utente (cifrati in PostgreSQL) e li usa per leggere Calendar e Cloud Identity; l'app riceve solo una sessione propria (access token JWT e refresh token, salvato nel keychain del sistema).

Il dettaglio di ogni schermata e di ogni chiamata è in [Schermate e flusso frontend-backend](./flusso-frontend-backend.md).

### Mappa della repository

| Percorso | Contenuto |
| --- | --- |
| `crates/core/` | `schisaciaccole-core`: logica pura condivisa (countdown, opzioni di orario, turni, Daily, righe dell'editor e preferenze) e contratti HTTP tra app e server (`api.rs`) |
| `crates/app/` | `schisaciaccole`: app desktop Rust/Slint. UI in `ui/*.slint`, stato in `src/state.rs`, sessione e worker in `src/session.rs`, client HTTP in `src/backend.rs`, login loopback in `src/login.rs` |
| `crates/server/` | `schisaciaccole-server`: server axum. Rotte in `src/app.rs`, OAuth e sessioni in `src/auth.rs`, meeting in `src/meetings.rs`, preferenze in `src/preferences.rs`, gruppi Google in `src/groups.rs` |
| `crates/server/migrations/` | Migrazioni PostgreSQL, applicate all'avvio del server |
| `.sqlx/` | Metadati offline di SQLx, usati dalla CI con `SQLX_OFFLINE=true` |
| `deploy/` | `Dockerfile` del server e `docker-compose.yml` con i servizi `db` (PostgreSQL 17) e `server` |
| `packaging/arch/` | `PKGBUILD` e file desktop per Arch Linux |
| `Dockerfile.deb` | Build del pacchetto `.deb` in Docker |
| `crates/app/assets/` | Icone e suono di fine meeting dell'app |
| `.github/workflows/` | `ci.yml` (formattazione, build, test, clippy, controllo dei dati SQLx) e `release.yml` (pacchetti `.deb`, Arch e macOS firmato, immagine del server) |
| `assets/` | Screenshot dell'app usati nel README e nella guida |
| `mockups/` | Mockup HTML del design, senza dipendenze |
| `.env.example` | Variabili d'ambiente del server da copiare in `.env` |
| `docs/` | Documentazione: guida in `docs/guide/`, design e decisioni in `docs/design/`, indice in `docs/README.md` |

Comandi di build, test, packaging e deploy: vedi il [README](../../README.md), in particolare [Build & run](../../README.md#build--run), [Backend (local development)](../../README.md#backend-local-development), [Tests](../../README.md#tests) e [Deploying the server](../../README.md#deploying-the-server).

Quando codice e documentazione divergono, vale il codice.

## Pagine di questa guida

- [Schermate e flusso frontend-backend](./flusso-frontend-backend.md): tutte le schermate dell'app desktop e le chiamate al server per login, meeting e preferenze dei meeting ([versione HTML](../flusso-frontend-backend.html))
