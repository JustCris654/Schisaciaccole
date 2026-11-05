CREATE TABLE users (
  id            UUID PRIMARY KEY,
  google_sub    TEXT NOT NULL UNIQUE,
  email         TEXT NOT NULL,
  display_name  TEXT,
  created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE google_credentials (
  user_id           UUID PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
  refresh_token_enc BYTEA NOT NULL,
  scopes            TEXT NOT NULL,
  updated_at        TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE app_sessions (
  id                 UUID PRIMARY KEY,
  user_id            UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  refresh_token_hash BYTEA NOT NULL UNIQUE,
  device_label       TEXT,
  expires_at         TIMESTAMPTZ NOT NULL,
  revoked_at         TIMESTAMPTZ,
  created_at         TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX app_sessions_user_id_idx ON app_sessions(user_id);
