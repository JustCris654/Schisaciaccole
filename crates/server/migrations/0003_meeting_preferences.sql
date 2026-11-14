DROP TABLE slot_template_entries;
DROP TABLE slot_templates;

CREATE TABLE meeting_preferences (
  id            UUID PRIMARY KEY,
  user_id       UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  meeting_key   TEXT NOT NULL,
  meeting_title TEXT NOT NULL,
  updated_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
  UNIQUE (user_id, meeting_key)
);

CREATE TABLE meeting_preference_people (
  preference_id  UUID NOT NULL REFERENCES meeting_preferences(id) ON DELETE CASCADE,
  position       INT  NOT NULL,
  kind           TEXT NOT NULL CHECK (kind IN ('attendee', 'manual')),
  attendee_email TEXT,
  label          TEXT NOT NULL,
  PRIMARY KEY (preference_id, position),
  CHECK ((kind = 'attendee') = (attendee_email IS NOT NULL))
);
