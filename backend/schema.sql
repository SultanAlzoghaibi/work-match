-- Co-working matcher app: database schema (SQLite)
--
-- Retention:
--   users, connections, cowork_sessions: permanent
--   messages: permanent for trial/friends, delete 1 day after a connection is ended
--   offers: short-lived, delete after about 1 day
--
-- Not stored here: presence (current activity, websocket, in_session cache,
-- pending_offer_id, last_declined_at). That lives in memory in presence.py.

PRAGMA foreign_keys = ON;

-- Who someone is. Created at onboarding.
CREATE TABLE users (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    token           TEXT    NOT NULL UNIQUE,              -- secret sent in "hello"
    name            TEXT    NOT NULL,
    school          TEXT,                                 -- shown in popup, matching context
    bio             TEXT,                                 -- one line, matching context
    sharing_enabled INTEGER NOT NULL DEFAULT 0 CHECK (sharing_enabled IN (0, 1)),
    created_at      TEXT    NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Relationship between two people. Starts as a trial after both accept an offer,
-- becomes friends if both opt in after the first session, otherwise ended.
CREATE TABLE connections (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    user_a            INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    user_b            INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    status            TEXT    NOT NULL DEFAULT 'trial'
                      CHECK (status IN ('trial', 'friends', 'ended')),
    a_wants_friend    INTEGER CHECK (a_wants_friend IN (0, 1)),   -- NULL until answered
    b_wants_friend    INTEGER CHECK (b_wants_friend IN (0, 1)),
    created_at        TEXT    NOT NULL DEFAULT CURRENT_TIMESTAMP,
    became_friends_at TEXT,
    ended_at          TEXT,                                       -- used for message cleanup
    CHECK (user_a < user_b)                                       -- store each pair one way
);

-- Only one live (trial or friends) connection per pair.
CREATE UNIQUE INDEX uq_connections_live_pair
    ON connections (user_a, user_b)
    WHERE status IN ('trial', 'friends');

-- A proposed match, shown as the corner popup.
CREATE TABLE offers (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    user_a        INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    user_b        INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    activity      TEXT    NOT NULL,                   -- shared detail for popup text
    score         REAL    NOT NULL CHECK (score BETWEEN 0 AND 1),   -- Jev probability
    a_status      TEXT    NOT NULL DEFAULT 'pending'
                  CHECK (a_status IN ('pending', 'accepted', 'declined')),
    b_status      TEXT    NOT NULL DEFAULT 'pending'
                  CHECK (b_status IN ('pending', 'accepted', 'declined')),
    created_at    TEXT    NOT NULL DEFAULT CURRENT_TIMESTAMP,       -- expires after 2 min
    connection_id INTEGER REFERENCES connections(id) ON DELETE SET NULL,  -- set when both accept
    CHECK (user_a <> user_b)
);

CREATE INDEX idx_offers_user_a     ON offers (user_a, created_at);
CREATE INDEX idx_offers_user_b     ON offers (user_b, created_at);
CREATE INDEX idx_offers_created_at ON offers (created_at);   -- cleanup job

-- One working block together (shared timer, goals, check-in).
CREATE TABLE cowork_sessions (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    connection_id INTEGER NOT NULL REFERENCES connections(id) ON DELETE CASCADE,
    started_at    TEXT    NOT NULL DEFAULT CURRENT_TIMESTAMP,
    duration_min  INTEGER NOT NULL DEFAULT 25 CHECK (duration_min > 0),
    ended_at      TEXT,                                   -- NULL = session is active
    end_reason    TEXT CHECK (end_reason IN ('timer', 'user_left', 'disconnected')),
    goal_a        TEXT,
    goal_b        TEXT,
    outcome_a     TEXT,
    outcome_b     TEXT
);

-- Only one active session per connection.
CREATE UNIQUE INDEX uq_sessions_one_active
    ON cowork_sessions (connection_id)
    WHERE ended_at IS NULL;

-- Chat messages between two connected people.
CREATE TABLE messages (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    connection_id INTEGER NOT NULL REFERENCES connections(id) ON DELETE CASCADE,
    session_id    INTEGER REFERENCES cowork_sessions(id) ON DELETE SET NULL,
    sender_id     INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    text          TEXT    NOT NULL,
    sent_at       TEXT    NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_messages_connection ON messages (connection_id, sent_at);

-- Useful queries ------------------------------------------------------------

-- Is a user in an active session? (matcher skips them)
-- SELECT 1 FROM cowork_sessions s
-- JOIN connections c ON c.id = s.connection_id
-- WHERE s.ended_at IS NULL AND (c.user_a = :uid OR c.user_b = :uid);

-- Did these two decline each other in the last day? (matcher skips the pair)
-- SELECT 1 FROM offers
-- WHERE ((user_a = :a AND user_b = :b) OR (user_a = :b AND user_b = :a))
--   AND 'declined' IN (a_status, b_status)
--   AND created_at > datetime('now', '-1 day');

-- Cleanup (run hourly, optional for the hackathon)
-- DELETE FROM offers WHERE created_at < datetime('now', '-1 day');
-- DELETE FROM messages WHERE connection_id IN (
--     SELECT id FROM connections WHERE status = 'ended' AND ended_at < datetime('now', '-1 day'));
