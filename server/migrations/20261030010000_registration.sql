CREATE TABLE registration_invites (
    code_hash BLOB PRIMARY KEY NOT NULL CHECK(length(code_hash) = 32),
    max_uses INTEGER NOT NULL DEFAULT 3 CHECK(max_uses = 3),
    used INTEGER NOT NULL DEFAULT 0 CHECK(used BETWEEN 0 AND max_uses),
    active INTEGER NOT NULL DEFAULT 1 CHECK(active IN (0, 1)),
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    last_used_at TEXT
);
ALTER TABLE users ADD COLUMN registration_invite_hash BLOB REFERENCES registration_invites(code_hash);

-- Independent of SQLite's AUTOINCREMENT and of manually chosen/existing UINs.
CREATE TABLE registration_state (
    id INTEGER PRIMARY KEY CHECK(id = 1),
    next_uin INTEGER NOT NULL CHECK(next_uin BETWEEN 112233 AND 16777216)
);
INSERT INTO registration_state(id, next_uin) VALUES (1, 112233);
