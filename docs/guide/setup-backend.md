# Backend Setup

The backend (`schisaciaccole-server`) is only needed if you want **Today's meetings** (`I miei meeting`) with Google Calendar and Google Groups. **Manual mode** (`Manuale`) works without a backend.

**Prerequisites:** Docker (with Compose), OpenSSL, a Google Workspace domain with Google Cloud Console access, and [Rust](https://www.rust-lang.org/tools/install) for local runs or building the desktop app.

## 1. Configure Google Cloud Console

1. Create (or select) a project inside your Google Cloud organization.
2. Go to **APIs & Services → Library** and enable:
   - **Google Calendar API**
   - **Cloud Identity API**
3. Configure the consent screen (**Google Auth Platform → Audience / Data Access**, or **APIs & Services → OAuth consent screen**):
   - User type: **Internal** (Google Workspace organization).
   - Scopes to include:
     - `openid`
     - `email`
     - `profile`
     - `https://www.googleapis.com/auth/calendar.events.owned.readonly` *(if this is not enough, use `https://www.googleapis.com/auth/calendar.events.readonly` and set `GOOGLE_CALENDAR_SCOPE` in `.env`)*
     - `https://www.googleapis.com/auth/cloud-identity.groups.readonly` *(expands invited Google Groups)*
4. Go to **Clients / Credentials → Create OAuth client ID**:
   - Application type: **Web application**
   - **Authorized redirect URI**: `<PUBLIC_BASE_URL>/auth/google/callback`
     - Local: `http://localhost:8080/auth/google/callback`
     - Production: `https://schisaciaccole.example.com/auth/google/callback`
5. Copy the **Client ID** and **Client secret**.

## 2. Configure `.env`

Create `.env` in the repository root (it is git-ignored):

```sh
cp .env.example .env
```

Generate two distinct keys for `TOKEN_ENC_KEY` and `JWT_SIGNING_KEY`:

```sh
openssl rand -base64 32
openssl rand -base64 32
```

Fill in the variables in `.env`:

| Variable | Required | Value |
| --- | --- | --- |
| `DATABASE_URL` | Yes | PostgreSQL URL (`postgres://schisaciaccole:schisaciaccole@localhost:5432/schisaciaccole` locally; Docker Compose overrides it for the `server` service) |
| `PUBLIC_BASE_URL` | Yes | Public server URL without a trailing `/` (`http://localhost:8080` locally, `https://...` in production) |
| `GOOGLE_CLIENT_ID` | Yes | OAuth Client ID from Google Cloud Console |
| `GOOGLE_CLIENT_SECRET` | Yes | OAuth Client secret from Google Cloud Console |
| `ALLOWED_HD` | Yes | Google Workspace domain allowed to sign in (e.g. `example.com`) |
| `TOKEN_ENC_KEY` | Yes | Base64 decoding to exactly 32 bytes (encrypts stored Google tokens) |
| `JWT_SIGNING_KEY` | Yes | At least 32 bytes (signs session JWTs) |
| `BIND_ADDR` | No | Listen address (default `0.0.0.0:8080`) |
| `TRUSTED_PROXY_HOPS` | No | Reverse proxies in front of the server appending to `X-Forwarded-For`: `0` when exposed directly (default), `1` behind one HTTPS reverse proxy |
| `GOOGLE_CALENDAR_SCOPE` | No | Calendar scope override (default `.../calendar.events.owned.readonly`) |

## 3. Run the Backend

PostgreSQL migrations run automatically on server startup.

### Local development

Start PostgreSQL 17, then run the server:

```sh
docker compose -f deploy/docker-compose.yml up -d db
cargo run -p schisaciaccole-server
```

### Docker Compose (server + database)

Build and start both services (reads `.env` from the repository root and exposes port `8080`):

```sh
docker compose -f deploy/docker-compose.yml up -d --build
```

In production, put an HTTPS reverse proxy in front of port `8080`, set `PUBLIC_BASE_URL=https://schisaciaccole.example.com`, and set `TRUSTED_PROXY_HOPS=1`.

## 4. Build the Desktop App Against the Backend

The backend URL is baked in at compile time via `SCHISACIACCOLE_BACKEND_URL` (when unset, the app stays in local-only mode):

```sh
SCHISACIACCOLE_BACKEND_URL=http://localhost:8080 cargo run --release -p schisaciaccole
```

For production builds, replace `http://localhost:8080` with your HTTPS `PUBLIC_BASE_URL`.
