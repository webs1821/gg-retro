-- Keep only private image identifiers. Classic GG fetches bytes from the live sender.
-- A separate migration also upgrades databases created by the experimental cache branch.
PRAGMA secure_delete=ON;
CREATE TABLE image_offers (
    message_id INTEGER NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    size INTEGER NOT NULL CHECK(size BETWEEN 1 AND 261120),
    crc32 INTEGER NOT NULL CHECK(crc32 BETWEEN 0 AND 4294967295),
    PRIMARY KEY(message_id, size, crc32)
);
INSERT INTO image_offers(message_id,size,crc32)
SELECT message_id,size,crc32 FROM message_images;
DROP TABLE message_images;
ALTER TABLE image_offers RENAME TO message_images;
CREATE INDEX message_images_identity ON message_images(size,crc32);
