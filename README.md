# Schisaciaccole

A countdown timer for **time-boxed Scrum ceremonies**.

Scrum ceremonies are time-boxed: regardless of when the team actually gets into
the meeting room, the meeting should end at its designated time. Schisaciaccole
makes that easy — instead of asking "how long do we have left?", you pick the
**wall-clock time the meeting must end** and the timer counts down precisely to
that moment.

## Screenshots

| Manuale: end time picked | Today's meetings |
| --- | --- |
| ![Manuale: end time picked](assets/screenshot_end_time.png) | ![Today's meetings](assets/screenshot_meetings.png) |

| Turni di parola | Daily mode |
| --- | --- |
| ![Turni di parola](assets/screenshot_speaking_slots.png) | ![Daily mode](assets/screenshot_daily.png) |

| Meeting timer | Timer |
| --- | --- |
| ![Meeting timer](assets/screenshot_meeting_timer.png) | ![Timer](assets/screenshot_timer.png) |

## Requirements

- [Rust](https://www.rust-lang.org/tools/install) (stable toolchain, `cargo`)

## Project layout

The repo is a Cargo workspace:

- `crates/core` — the `schisaciaccole-core` library: shared time logic and API types.
- `crates/app` — the `schisaciaccole` binary: Slint desktop app (`ui/`), `build.rs`,
  assets (`assets/`: icons, sound, font) and the bundle metadata.
- `crates/server` — the `schisaciaccole-server` binary: axum backend with Google
  login, app sessions, meetings from Google Calendar and slot templates and plans
  (not used by the desktop app).
- `deploy/` — `Dockerfile` for the server and `docker-compose.yml` (server and
  PostgreSQL 17).
- `assets/` — screenshots used by this README.

## Build & run

```sh
cargo run -p schisaciaccole
```

For a release build:

```sh
cargo run -p schisaciaccole --release
```

Checks CI runs on every push and pull request (the server tests need PostgreSQL,
see [Tests](#tests)):

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
```

## Packaging

The app icons live at `crates/app/assets/icon_{128,256,512}.png` and the
`crates/app/assets/arcade_victory.mp3` sound ships as a resource. At
runtime the sound is located relative to the executable, so it works from
every package layout (dev, deb, macOS app, Arch pkg).

### Linux (.deb) and macOS (.app) — cargo-bundle

Run cargo-bundle from the app crate, not from the workspace root: with
`cargo bundle -p schisaciaccole` at the root it resolves the icon and resource
paths against the current directory and silently leaves them out of the
bundle.

```sh
cargo install --locked cargo-bundle   # once

cd crates/app
cargo bundle --release --format deb   # Linux  -> target/release/bundle/deb/*.deb
cargo bundle --release --format osx   # macOS  -> *.app (must run on macOS)
```

The bundles land in the workspace `target/` dir at the repo root. Bundle
metadata (name, identifier, icon, bundled resources) lives in the
`[package.metadata.bundle]` section of `crates/app/Cargo.toml`.

### Linux (.deb) via Docker

Building the `.deb` directly needs `cargo-bundle` plus a handful of native
dev libraries (ALSA, fontconfig, xcb...). `Dockerfile.deb` sets up a
throwaway Ubuntu 22.04 image with Rust, `cargo-bundle` and those libraries
pre-installed, so you don't need any of that on the host.

Build the image once:

```sh
docker build -t deb-builder -f Dockerfile.deb .
```

Then run the bundle build, mounting the repo into the container:

```sh
docker run --rm \
    --user $(id -u):$(id -g) \
    -e HOME=/tmp \
    -e CARGO_HOME=/tmp/cargo \
    -v "$(pwd)":/app \
    -w /app/crates/app \
    deb-builder cargo bundle --release --format deb
```

- `--user $(id -u):$(id -g)` makes the container write files as your host
  user instead of root.
- `HOME` and `CARGO_HOME` are redirected to `/tmp` because the mounted-in
  user has no home directory inside the container.

The resulting package lands at `target/release/bundle/deb/*.deb`, same as
the native `cargo bundle` build.

### Arch Linux (pacman)

> **Run `makepkg` from inside `packaging/arch/`, never from the repo root.**
> makepkg uses `./src` and `./pkg` as its work dirs and the PKGBUILD finds the
> repo two levels above its own directory. It builds a clean copy of the
> workspace (`Cargo.toml`, `Cargo.lock`, `crates/`) under its own `src/`, so
> the sources are never touched.

```sh
cd packaging/arch
makepkg -f                  # rustup users without the pacman 'cargo' pkg: makepkg -fd
sudo pacman -U schisaciaccole-*.pkg.tar.zst
```

This installs the binary, the sound, the icon, a `.desktop` launcher and the
license under the standard `/usr` paths.

### Releases (CI)

Pushing a tag `vX.Y.Z` that matches the `version` in the
`[workspace.package]` section of the root `Cargo.toml` triggers
`.github/workflows/release.yml`, which builds the `.deb`, the Arch package
and a macOS `.app` (as a `.zip` and a `.dmg`, unsigned unless the signing
secrets below are set) and attaches them all to a GitHub Release:

```sh
git tag v0.2.0
git push origin v0.2.0
```

A mismatched tag (e.g. tagging `v0.2.1` while `Cargo.toml` says `0.2.0`)
fails the run before anything is built. The workflow can also be triggered
manually (`workflow_dispatch`) to build and upload all three packages as
run artifacts without cutting a release.

Two repository settings affect the release:

- The repository variable `SCHISACIACCOLE_BACKEND_URL` is the backend URL baked
  into the desktop builds (see
  [Building the app against a backend](#building-the-app-against-a-backend)).
  Unset means local-only builds.
- The optional macOS signing secrets. Set all seven or none: a partial set fails
  the run instead of producing an unsigned build. Without them the macOS build
  is unsigned. The names are:
  - `APPLE_CERTIFICATE_P12_BASE64`
  - `APPLE_CERTIFICATE_PASSWORD`
  - `APPLE_SIGNING_IDENTITY`
  - `APPLE_TEAM_ID`
  - `APPLE_API_KEY_ID`
  - `APPLE_API_ISSUER_ID`
  - `APPLE_API_KEY_P8_BASE64`

Tag pushes also build and push the server image to GHCR as
`ghcr.io/justcris654/schisaciaccole-server:<version>` and
`ghcr.io/justcris654/schisaciaccole-server:latest`, where `<version>` is the
version without the `v`. The first push creates a private package: make it
public, or log in to GHCR on the host that pulls it.

## Backend (local development)

The server is the `schisaciaccole-server` crate in `crates/server`. It needs a
PostgreSQL database and a `.env` file in the repository root (the server loads
it at startup).

Copy the example and fill it in:

```sh
cp .env.example .env
```

Generate the secrets with:

```sh
openssl rand -base64 32
```

- `TOKEN_ENC_KEY` must be base64 that decodes to exactly 32 bytes (it encrypts
  the stored Google tokens).
- `JWT_SIGNING_KEY` must be at least 32 characters (it signs the server's JWTs).

Start PostgreSQL 17 from the compose file:

```sh
docker compose -f deploy/docker-compose.yml up -d db
```

Run the server:

```sh
cargo run -p schisaciaccole-server
```

The server listens on `BIND_ADDR` (default `0.0.0.0:8080`), runs the database
migrations at startup, and needs the Google OpenID discovery document
(`GOOGLE_ISSUER_URL`, default `https://accounts.google.com`) to be reachable.

### Google Cloud setup (real login)

A real login needs a Google OAuth client. Create a project inside the company
Google Cloud organization, then:

- Enable the **Google Calendar API**.
- Configure the **OAuth consent screen** with user type **Internal** and the scopes
  `openid`, `email`, `profile` and
  `https://www.googleapis.com/auth/calendar.events.owned.readonly`. If that scope
  is not enough, use `https://www.googleapis.com/auth/calendar.events.readonly`
  instead and set `GOOGLE_CALENDAR_SCOPE` to it.
- Create an **OAuth client ID** of type **Web application** with the authorized
  redirect URI `<PUBLIC_BASE_URL>/auth/google/callback` (locally:
  `http://localhost:8080/auth/google/callback`).
- Put the client ID and secret in `.env` as `GOOGLE_CLIENT_ID` and
  `GOOGLE_CLIENT_SECRET`, and set `ALLOWED_HD` to the company domain.

## Tests
The server tests need a running PostgreSQL. `#[sqlx::test]` creates a throwaway
database per test, so the user in `DATABASE_URL` must be allowed to create
databases (the compose database user is the superuser).

```sh
DATABASE_URL=postgres://schisaciaccole:schisaciaccole@localhost:5432/schisaciaccole cargo test --workspace
```

SQL queries are checked at compile time with sqlx. After changing a query or a
migration, install the sqlx CLI once, apply the migrations and regenerate the
offline data:

```sh
cargo install sqlx-cli --version 0.8.6 --no-default-features --features rustls,postgres --locked   # once
sqlx migrate run --source crates/server/migrations   # with DATABASE_URL set, as in CI
cargo sqlx prepare --workspace
```

Commit the `.sqlx/` directory. Builds without a database use `.sqlx/`
automatically; `SQLX_OFFLINE=true` forces that.

## Building the app against a backend
The desktop app talks to the backend through a URL fixed at compile time:

```sh
SCHISACIACCOLE_BACKEND_URL=https://schisaciaccole.example.com cargo build --release -p schisaciaccole
```

Without the variable the app is the local-only quarter-hour timer. The session
refresh token is stored in the OS keychain (macOS Keychain, Linux Secret
Service).

## Deploying the server

The server runs from the image published by the release workflow, next to a
PostgreSQL 17 database, as defined in `deploy/docker-compose.yml`.

On the host, check out the repository and put the real `.env` in its root:
compose reads `../.env` relative to `deploy/`. Then start the stack with the
image tag, which is the version without the `v`:

```sh
SERVER_TAG=<version> docker compose -f deploy/docker-compose.yml pull server
SERVER_TAG=<version> docker compose -f deploy/docker-compose.yml up -d
```

- The `server` service uses `ghcr.io/justcris654/schisaciaccole-server:${SERVER_TAG}`
  (`latest` if `SERVER_TAG` is unset) and publishes port 8080. Its
  `DATABASE_URL` is overridden to point to the `db` service.
- `TRUSTED_PROXY_HOPS` tells the server how many reverse proxies sit in front
  of it, for the per-client rate limit on `/auth` and the per-client cap on
  logins in progress. Leave it at `0` (the default) when the server is exposed
  directly: forwarding headers are ignored and the client is the peer address.
  Set it to `1` behind one reverse proxy with HTTPS that appends the client
  address to `X-Forwarded-For` (nginx `$proxy_add_x_forwarded_for`): the last
  entry of the header is the client. Never set it higher than the number of
  proxies, or clients can pick their own address.
- `PUBLIC_BASE_URL` must be the public HTTPS URL of the server.
- The server serves at most 1024 connections at once and stops accepting
  while at the cap. It closes a connection that does not send complete
  request headers within 10 seconds or stays 10 seconds with no request in
  handling, and answers 408 to a request (body included) that takes more
  than 30 seconds. On `SIGTERM` it waits at most 30 seconds for open
  requests before exiting.

## Logging

The app uses `env_logger`. Control verbosity with the `RUST_LOG` environment
variable (default is `warn`):

```sh
RUST_LOG=info cargo run -p schisaciaccole     # callback events: selection, start, stop, sound
RUST_LOG=debug cargo run -p schisaciaccole    # also the periodic option refresh
```

## License

Schisaciaccole is released under the
[PolyForm Noncommercial License 1.0.0](https://polyformproject.org/licenses/noncommercial/1.0.0)
(see `LICENSE`). Noncommercial use only: commercial use requires the author's
written permission. Copyright (c) 2026 Cristian Scapin.

The UI is built with [Slint](https://slint.dev), used under the Slint Royalty-free license 2.0.
