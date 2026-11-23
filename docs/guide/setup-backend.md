# Setup del backend

Il backend (`schisaciaccole-server`) serve solo se vuoi usare **I miei meeting** con Google Calendar e Google Groups. La modalità **Manuale** funziona senza backend.

**Prerequisiti:** Docker (con Compose), OpenSSL, un dominio Google Workspace con accesso a Google Cloud Console e [Rust](https://www.rust-lang.org/tools/install) per l'avvio locale o la compilazione dell'app.

## 1. Configura Google Cloud Console

1. Crea (o seleziona) un progetto nella tua organizzazione Google Cloud.
2. Vai su **APIs & Services → Library** e abilita:
   - **Google Calendar API**
   - **Cloud Identity API**
3. Configura la schermata di consenso (**Google Auth Platform → Audience / Data Access**, oppure **APIs & Services → OAuth consent screen**):
   - Tipo utente: **Internal** (organizzazione Google Workspace).
   - Scope da includere:
     - `openid`
     - `email`
     - `profile`
     - `https://www.googleapis.com/auth/calendar.events.owned.readonly` *(se non basta, usa `https://www.googleapis.com/auth/calendar.events.readonly` e imposta `GOOGLE_CALENDAR_SCOPE` in `.env`)*
     - `https://www.googleapis.com/auth/cloud-identity.groups.readonly` *(per espandere i Google Groups invitati)*
4. Vai su **Clients / Credentials → Create OAuth client ID**:
   - Tipo applicazione: **Web application**
   - **Authorized redirect URI**: `<PUBLIC_BASE_URL>/auth/google/callback`
     - Locale: `http://localhost:8080/auth/google/callback`
     - Produzione: `https://schisaciaccole.example.com/auth/google/callback`
5. Copia **Client ID** e **Client secret**.

## 2. Configura `.env`

Crea `.env` alla radice del repository (è ignorato da git):

```sh
cp .env.example .env
```

Genera due chiavi distinte per `TOKEN_ENC_KEY` e `JWT_SIGNING_KEY`:

```sh
openssl rand -base64 32
openssl rand -base64 32
```

Compila le variabili in `.env`:

| Variabile | Obbligatoria | Valore |
| --- | --- | --- |
| `DATABASE_URL` | Sì | URL PostgreSQL (`postgres://schisaciaccole:schisaciaccole@localhost:5432/schisaciaccole` in locale; Docker Compose la sovrascrive per il servizio `server`) |
| `PUBLIC_BASE_URL` | Sì | URL pubblico del server senza `/` finale (`http://localhost:8080` in locale, `https://...` in produzione) |
| `GOOGLE_CLIENT_ID` | Sì | Client ID OAuth da Google Cloud Console |
| `GOOGLE_CLIENT_SECRET` | Sì | Client secret OAuth da Google Cloud Console |
| `ALLOWED_HD` | Sì | Dominio Google Workspace ammesso al login (es. `example.com`) |
| `TOKEN_ENC_KEY` | Sì | Base64 di esattamente 32 byte (cifra i token Google salvati) |
| `JWT_SIGNING_KEY` | Sì | Almeno 32 byte (firma i JWT di sessione) |
| `BIND_ADDR` | No | Indirizzo di ascolto (default `0.0.0.0:8080`) |
| `TRUSTED_PROXY_HOPS` | No | Proxy davanti al server che aggiornano `X-Forwarded-For`: `0` se esposto diretto (default), `1` dietro un reverse proxy HTTPS |
| `GOOGLE_CALENDAR_SCOPE` | No | Override dello scope Calendar (default `.../calendar.events.owned.readonly`) |

## 3. Avvia il backend

Le migrazioni PostgreSQL vengono applicate automaticamente all'avvio del server.

### Sviluppo locale

Avvia PostgreSQL 17 e poi il server:

```sh
docker compose -f deploy/docker-compose.yml up -d db
cargo run -p schisaciaccole-server
```

### Docker Compose (server + database)

Costruisci e avvia entrambi i servizi (legge `.env` alla radice del repository ed espone la porta `8080`):

```sh
docker compose -f deploy/docker-compose.yml up -d --build
```

In produzione usa un reverse proxy con HTTPS davanti alla porta `8080`, imposta `PUBLIC_BASE_URL=https://schisaciaccole.example.com` e `TRUSTED_PROXY_HOPS=1`.

## 4. Compila l'app desktop con il backend

L'URL del backend viene fissato a compilazione tramite `SCHISACIACCOLE_BACKEND_URL` (senza la variabile l'app resta in modalità locale):

```sh
SCHISACIACCOLE_BACKEND_URL=http://localhost:8080 cargo run --release -p schisaciaccole
```

Per una build verso produzione, sostituisci `http://localhost:8080` con il tuo `PUBLIC_BASE_URL` HTTPS.
