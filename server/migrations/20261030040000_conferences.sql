-- Legacy GG conferences are per-recipient messages carrying the other participants.
ALTER TABLE messages ADD COLUMN conference BLOB
    CHECK(conference IS NULL OR (typeof(conference)='blob' AND length(conference) BETWEEN 9 AND 77));
