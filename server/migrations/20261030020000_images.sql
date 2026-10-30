-- Image contents are private attachments to one accepted message, never a public CRC cache.
CREATE TABLE message_images (
    message_id INTEGER NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    size INTEGER NOT NULL CHECK(size BETWEEN 1 AND 261120),
    crc32 INTEGER NOT NULL CHECK(crc32 BETWEEN 0 AND 4294967295),
    last_attempt INTEGER NOT NULL DEFAULT 0,
    filename BLOB,
    data BLOB,
    PRIMARY KEY(message_id, size, crc32),
    CHECK((data IS NULL AND filename IS NULL) OR
          (data IS NOT NULL AND filename IS NOT NULL AND length(data)=size AND length(filename) BETWEEN 1 AND 1024))
);
CREATE INDEX message_images_identity ON message_images(size, crc32);
CREATE INDEX messages_image_participants ON messages(sender_uin, recipient_uin, id);
