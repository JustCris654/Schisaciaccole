CREATE TABLE slot_templates (
  id            UUID PRIMARY KEY,
  user_id       UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  meeting_key   TEXT NOT NULL,
  meeting_title TEXT NOT NULL,
  updated_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
  UNIQUE (user_id, meeting_key)
);

CREATE TABLE slot_template_entries (
  template_id    UUID NOT NULL REFERENCES slot_templates(id) ON DELETE CASCADE,
  position       INT  NOT NULL,
  kind           TEXT NOT NULL CHECK (kind IN ('attendee', 'manual', 'free_time')),
  attendee_email TEXT,
  label          TEXT NOT NULL,
  weight_secs    INT  NOT NULL CHECK (weight_secs >= 0),
  PRIMARY KEY (template_id, position),
  CHECK ((kind = 'attendee') = (attendee_email IS NOT NULL))
);
