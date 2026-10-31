-- Opt-in only. Registration names/emails are never copied to public profiles.
CREATE TABLE public_profiles (
    uin INTEGER PRIMARY KEY REFERENCES users(uin) ON DELETE CASCADE,
    firstname TEXT NOT NULL DEFAULT '' CHECK(length(firstname) <= 50),
    lastname TEXT NOT NULL DEFAULT '' CHECK(length(lastname) <= 50),
    nickname TEXT NOT NULL DEFAULT '' CHECK(length(nickname) <= 50),
    city TEXT NOT NULL DEFAULT '' CHECK(length(city) <= 50),
    familyname TEXT NOT NULL DEFAULT '' CHECK(length(familyname) <= 50),
    familycity TEXT NOT NULL DEFAULT '' CHECK(length(familycity) <= 50),
    birthyear INTEGER NOT NULL DEFAULT 0 CHECK(birthyear BETWEEN 0 AND 9999),
    gender INTEGER NOT NULL DEFAULT 0 CHECK(gender BETWEEN 0 AND 2),
    first_key TEXT NOT NULL DEFAULT '',
    last_key TEXT NOT NULL DEFAULT '',
    nick_key TEXT NOT NULL DEFAULT '',
    city_key TEXT NOT NULL DEFAULT '',
    familyname_key TEXT NOT NULL DEFAULT '',
    familycity_key TEXT NOT NULL DEFAULT ''
);
