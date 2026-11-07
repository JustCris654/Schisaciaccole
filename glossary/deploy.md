# Deploy

I due modi per far girare il server e il database. Allineato al commit `5f6a1dc6` di cartography.

## Sviluppo locale: docker compose

File `deploy/docker-compose.yml`, con i servizi `db` e `server`.

- `db`: immagine `postgres:17`, utente/password/database `schisaciaccole`, volume `pgdata`, porta legata solo a `127.0.0.1:5432`.
- `server`: immagine `ghcr.io/justcris654/schisaciaccole-server:${SERVER_TAG:-latest}`, costruita da `deploy/Dockerfile`, legge `../.env`, `DATABASE_URL` sovrascritta per puntare a `db`, porta `8080`.

Solo il database: `docker compose -f deploy/docker-compose.yml up -d db`. Stack completo: `SERVER_TAG=<versione> docker compose -f deploy/docker-compose.yml up -d`. Le variabili sono documentate in `.env.example`.

## Cluster utilities: chart Helm in cartography

Il chart sta solo nel repo cartography, in `helm/0.company/schisaciaccole/` (non in questo repo): questo repo fornisce l'immagine, cartography il chart e i valori del cluster.

| Risorsa | Tipo | Note |
| --- | --- | --- |
| `schisaciaccole-secrets` | Secret | Opaque (non ExternalSecret, ambiente non prod): `GOOGLE_CLIENT_ID`, `GOOGLE_CLIENT_SECRET`, `TOKEN_ENC_KEY`, `JWT_SIGNING_KEY`, `POSTGRES_PASSWORD` |
| `postgresql` | StatefulSet + Service ClusterIP | `postgres:17`, PVC 5Gi sulla storage class di default, porta 5432, mai esposto fuori dal cluster |
| `schisaciaccole-server` | Deployment | 1 replica, strategia `Recreate` (i login in corso stanno in memoria nel `LoginStore`), initContainer che aspetta Postgres con `pg_isready`, probe su `/healthz` |
| `schisaciaccole-server-service` | Service ClusterIP | porta 8080 |
| `schisaciaccole-server-http-route` | HTTPRoute | host `api-schisaciaccole.openmove.com` sul listener `https-main` di `main-gateway` (namespace `envoy-gateway-system`), certificato wildcard `*.openmove.com` |

Immagine `ghcr.io/justcris654/schisaciaccole-server:<versione>`, usata come riferimento completo, senza il prefisso del registry GCP `commonValues.artifacts.repo`; il package ghcr deve essere pubblico (niente `imagePullSecret`).

`DATABASE_URL` è composta nel Deployment come `postgres://schisaciaccole:$(POSTGRES_PASSWORD)@postgresql:5432/schisaciaccole`, quindi `POSTGRES_PASSWORD` deve essere URL-safe (generata con `openssl rand -hex 24`).

File: `values.yaml` (default condivisi, senza segreti: namespace `schisaciaccole`, porte, risorse, `BIND_ADDR`, `ALLOWED_HD`) e `envs/utilities/utilities-values.yaml` (tag dell'immagine, host, `PUBLIC_BASE_URL`, segreti in chiaro, codificati in base64 dal template). `Taskfile.yml` definisce `UTILITIES_VALUES`, che rende il chart applicabile su utilities.

### Aggiornare la versione

Cambiare `server.image` in `envs/utilities/utilities-values.yaml` in cartography al nuovo tag pubblicato dal workflow di release, fare commit, poi il deploy.

### Deploy

```sh
cd <cartography>/helm/0.company/schisaciaccole
task update-deps
task upgrade-utilities
```

`upgrade-utilities` forza il contesto kube `utilities` a prescindere da quello corrente e chiede conferma y/N. Render senza applicare (equivalente a `task debug DIFF=false`, senza cambiare il contesto kube):

```sh
helm template schisaciaccole . --namespace schisaciaccole --set environment=utilities -f ../../_common/commonValues.yaml -f values.yaml -f envs/utilities/utilities-values.yaml
```

### Prerequisiti fuori dai repo

1. Package `schisaciaccole-server` pubblico su GitHub.
2. Record DNS A `api-schisaciaccole.openmove.com` → `34.12.113.247` (niente DNS wildcard su openmove.com).
3. Redirect URI `https://api-schisaciaccole.openmove.com/auth/google/callback` nel client OAuth Google.
4. Variabile del repo GitHub `SCHISACIACCOLE_BACKEND_URL=https://api-schisaciaccole.openmove.com`: il workflow di release la incorpora nel client desktop a tempo di compilazione.
