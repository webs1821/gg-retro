# GG 6.1 directory compatibility

The implementation targets the original GG 6.1.0.158 client. Its receive
dispatcher uses different operation codes in requests and replies:

| Operation | Request | Successful reply | Failed reply |
| --- | --- | --- | --- |
| Write own profile | 1 | 1 | 2 |
| Read own profile | 2 | 3 | 4 |
| Search | 3 | 5 | 6 |

Version 0.8.0 echoed READ operation 2 in its reply, following the simplified
libgadu protocol description. Native GG routed that reply to its write-failure
handler, leaving the profile-read progress window open. Version 0.8.1 sends 3.

This was verified against the native executable's directory dispatcher at
`0x4267c7`, particularly the READ branches at `0x4268b4` and `0x4268e5`, and
the WRITE branch at `0x426905`. The observed live exchange was a five-byte
READ request followed immediately by a complete reply with the same sequence;
network availability and response length were not the cause.

The native READ constructor at `0x4248bc` and WRITE constructor at `0x4248e2`
leave the sequence bytes unset. SEARCH at `0x424932` initializes and uses them.
READ/WRITE must therefore accept and echo arbitrary sequence values, including
zero; SEARCH still requires a nonzero sequence.

The profile form serializer at `0x4299ba` omits empty fields. A WRITE represents
the entire form, not a partial update. Omitted fields must be cleared, and an
empty form (possibly retaining a gender selection) removes the public profile.
The authenticated session remains the only authority for profile ownership.

Inspection used the user-supplied, locally patched client with SHA-256
`1db383709d5ba22f52f21f049697cbc70865db57f034489a2b774cf23944469d`.
No client binary changes are needed for this server fix. TCP integration tests
assert the native reply codes and exercise unset sequences, omitted fields,
profile removal and existing ownership/privacy protections.
