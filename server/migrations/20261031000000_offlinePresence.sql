-- Only an explicitly selected offline description is retained, never online/invisible state.
CREATE TABLE offline_presence (
  uin INTEGER PRIMARY KEY REFERENCES users(uin) ON DELETE CASCADE,
  status INTEGER NOT NULL CHECK(status IN (21, 32789)),
  description TEXT NOT NULL CHECK(length(description) BETWEEN 1 AND 70),
  return_time INTEGER CHECK(return_time BETWEEN 0 AND 4294967295),
  privacy BLOB NOT NULL CHECK(typeof(privacy)='blob' AND length(privacy)<=5000 AND length(privacy)%5=0)
);
