---
paths:
  - "deploy/**"
  - ".env.example"
---

# Server deployment

The supported repository deployment is `deploy/docker-compose.yml`: PostgreSQL 17 (`db`), server, persistent `pgdata`. The database is bound to loopback on the host; server publishes 8080. Compose reads root `.env` via `../.env` and overrides server `DATABASE_URL` to the `db` hostname.

The server image is `ghcr.io/justcris654/schisaciaccole-server:${SERVER_TAG:-latest}`. Follow `README.org` for the selected release version, pull/up commands, OAuth callback/public HTTPS URL, proxy hops and key requirements. Keep health dependencies and storage intact. Review migrations and data impact before altering startup/deployment.

Validate configuration without exposing resolved secrets. Do not deploy, remove volumes, restart a live stack or publish images unless requested. Update operational docs for changed services, ports, variables, volumes or procedures.
