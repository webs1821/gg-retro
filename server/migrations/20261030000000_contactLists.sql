CREATE TABLE contact_lists (
    uin INTEGER PRIMARY KEY REFERENCES users(uin) ON DELETE CASCADE,
    data BLOB NOT NULL,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX idx_messages_pending_order ON messages(recipient_uin, id) WHERE delivered_at IS NULL;
