"""Real TCP/HTTP integration tests using only temporary synthetic accounts. Set GG_RETRO_BINARY."""
import contextlib
import hashlib
import json
import os
from pathlib import Path
import socket
import sqlite3
import struct
import subprocess
import time
import traceback
import urllib.parse
import urllib.request

import tempfile
BASE = Path(__file__).resolve().parent
RUN = Path(tempfile.mkdtemp(prefix="gg-retro-wire-"))
DB = RUN / "synthetic.sqlite"
EXE = Path(os.environ["GG_RETRO_BINARY"]).resolve()
PASSWORD = "Synthetic123!"
INVITE = "synthetic-invitation-for-tests-only-2026"
(RUN / "password.key").write_bytes(os.urandom(32))
(RUN / "invite.txt").write_text(INVITE)
os.chmod(RUN / "password.key", 0o600)
os.chmod(RUN / "invite.txt", 0o600)
SENDER, RECIPIENT = 0, 0
RESULTS = []
PROC = None
LOG = (RUN / "server.log").open("wb")

def free_port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]

PORT, HTTP_PORT = free_port(), free_port()

def record(name, ok, **detail):
    row = {"test": name, "passed": bool(ok), **detail}
    RESULTS.append(row)
    print(json.dumps(row, ensure_ascii=True), flush=True)
    assert ok, name

def start(migrate=False):
    global PROC
    env = dict(os.environ, GG_BIND="127.0.0.1", GG_GG_PORT=str(PORT),
               GG_HTTP_PORT=str(HTTP_PORT), GG_DB=str(DB), GG_HOSTNAME="127.0.0.1",
               GG_ADVERTISED_IP="203.0.113.10", GG_ADVERTISED_PORT=str(PORT),
               GG_PASSWORD_KEY_FILE=str(RUN / "password.key"), GG_REGISTRATION_INVITE_FILE=str(RUN / "invite.txt"),
               GG_MIGRATE_PLAINTEXT_PASSWORDS=str(migrate).lower())
    PROC = subprocess.Popen([str(EXE)], cwd=RUN, env=env, stdout=LOG, stderr=LOG,
                            creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        if PROC.poll() is not None:
            raise RuntimeError(f"Server exited: {PROC.returncode}; inspect {RUN / 'server.log'}")
        try:
            with socket.create_connection(("127.0.0.1", PORT), timeout=.2):
                return
        except OSError:
            time.sleep(.05)
    raise RuntimeError("Local test server did not start")

def stop():
    global PROC
    if PROC is not None:
        PROC.kill()
        PROC.wait(timeout=10)
        PROC = None

def dbquery(query, args=()):
    with sqlite3.connect(DB, timeout=5) as conn:
        return conn.execute(query, args).fetchall()

def count_pending():
    return dbquery("SELECT count(*) FROM messages WHERE recipient_uin=? AND delivered_at IS NULL", (RECIPIENT,))[0][0]

def wait_pending(n):
    deadline = time.monotonic() + 5
    while time.monotonic() < deadline:
        actual = count_pending()
        if actual == n:
            return actual
        time.sleep(.02)
    raise AssertionError(f"Pending {actual}, expected {n}")

def hash_password(seed):
    x, y = 0, seed
    for b in PASSWORD.encode():
        x = (x & 0xffffff00) | b
        y ^= x
        y = (y + x) & 0xffffffff
        x = (x << 8) & 0xffffffff
        y ^= x
        x = (x << 8) & 0xffffffff
        y = (y - x) & 0xffffffff
        x = (x << 8) & 0xffffffff
        y ^= x
        z = y & 31
        y = ((y << z) | (y >> ((32-z) % 32))) & 0xffffffff
    return y

class Client:
    def __init__(self, uin, contacts=True, fragmented=False, status=2, image_size=255, description=None, return_time=None):
        self.sock = socket.create_connection(("127.0.0.1", PORT), timeout=5)
        self.sock.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
        kind, body = self.recv()
        assert kind == 1
        seed, = struct.unpack("<I", body)
        self.seed = seed
        payload = struct.pack("<IIIIB4sH4sHBB", uin, hash_password(seed), status,
                              0x20, 0, bytes(4), 0, bytes(4), 0, image_size, 0xbe)
        if description is not None:
            payload += description.encode('cp1250') + b'\0'
            if return_time is not None: payload += struct.pack('<I', return_time)
        self.send(0x15, payload, fragmented)
        assert self.until(3)[0] == 3
        if contacts:
            self.send(0x12)

    def send(self, kind, payload=b"", fragmented=False):
        wire = struct.pack("<II", kind, len(payload)) + payload
        if fragmented:
            for chunk in (wire[:3], wire[3:8], wire[8:10], wire[10:]):
                if chunk:
                    self.sock.sendall(chunk)
                    time.sleep(.03)
        else:
            self.sock.sendall(wire)

    def exact(self, size):
        data = b""
        while len(data) < size:
            chunk = self.sock.recv(size-len(data))
            if not chunk:
                raise EOFError("Server closed connection")
            data += chunk
        return data

    def recv(self):
        kind, length = struct.unpack("<II", self.exact(8))
        return kind, self.exact(length)

    def until(self, kind):
        for _ in range(2000):
            item = self.recv()
            if item[0] == kind:
                return item
        raise AssertionError("Too many intervening packets")

    def message(self, text, seq=1, recipient=None, formatting=b"", fragmented=False):
        if recipient is None:
            recipient = RECIPIENT
        self.send(11, struct.pack("<III", recipient, seq, 8) + text.encode("cp1250") + b"\0" + formatting, fragmented)
        _, ack = self.until(5)
        return struct.unpack("<III", ack)

    def receive_message(self):
        _, body = self.until(10)
        sender, seq, timestamp, klass = struct.unpack("<IIII", body[:16])
        content, _, formatting = body[16:].partition(b"\0")
        return dict(sender=sender, seq=seq, timestamp=timestamp, klass=klass,
                    text=content.decode("cp1250"), formatting=formatting.hex())

    def messages_until_pong(self):
        self.send(8)
        msgs = []
        while True:
            kind, data = self.recv()
            if kind == 7:
                return msgs
            if kind == 10:
                msgs.append(data)

    def close(self):
        with contextlib.suppress(OSError):
            self.sock.shutdown(socket.SHUT_RDWR)
        self.sock.close()
        time.sleep(.08)


def http(path, form=None):
    data = urllib.parse.urlencode(form).encode() if form is not None else None
    with urllib.request.urlopen(f"http://127.0.0.1:{HTTP_PORT}" + path, data=data, timeout=5) as response:
        return response.read().decode()

def registration(email, invite=INVITE):
    token = http("/appsvc/regtoken.asp").splitlines()[1]
    code = dbquery("SELECT captcha_code FROM tokens WHERE token_id=?", (token,))[0][0]
    form = dict(email=email, pwd=PASSWORD, invite=invite, tokenid=token, tokenval=code)
    return form, http("/appsvc/fmregister3.asp", form)

def watch(client, uin, flags=3):
    client.send(0x10, struct.pack("<IB", uin, flags))
    _, response = client.until(0x11)
    return response[4] if response else None

def main():
    global SENDER, RECIPIENT
    start()
    form, reply = registration("sender@example.invalid")
    assert reply.startswith("reg_success:"); SENDER = int(reply.split(":")[1])
    _, reply = registration("recipient@example.invalid")
    assert reply.startswith("reg_success:"); RECIPIENT = int(reply.split(":")[1])
    record("registration_handler_and_encrypted_credentials", all(PASSWORD not in p and p.startswith("ggv1:") for (p,) in dbquery("SELECT password FROM users")))
    record("captcha_is_one_time", http("/appsvc/fmregister3.asp", form) == "reg_failed:captcha")
    _, reply = registration("stranger@example.invalid", "wrong")
    record("registration_requires_invitation", reply == "reg_failed:invite" and dbquery("SELECT count(*) FROM users")[0][0] == 2)
    record("public_discovery_uses_configured_endpoint", http("/appsvc/appmsg4.asp").strip() == f"0 0 203.0.113.10:{PORT} 203.0.113.10")
    record("password_recovery_fails_without_panic", http("/appsvc/fmsendpwd3.asp", dict(userid=SENDER, tokenid="x", tokenval="x",code="x")) == "pwdsend_failed")
    a = Client(SENDER, fragmented=True)
    record("fragmented_login", True)
    expected = ["Pierwsza offline", "Zażółć gęślą jaźń", "Trzecia offline"]
    before = int(time.time())
    acknowledgements = [a.message(text, i+1) for i,text in enumerate(expected)]
    wait_pending(3)
    record("offline_saved_before_ack", all(x[0] == 3 for x in acknowledgements))
    a.close(); stop()
    record("database_survives_process_kill", count_pending() == 3 and dbquery("PRAGMA integrity_check")[0][0] == "ok")
    start(); b = Client(RECIPIENT, contacts=False)
    record("offline_waits_for_contact_list", not b.messages_until_pong() and count_pending() == 3)
    b.send(0x12); received = [b.receive_message() for _ in expected]; wait_pending(0)
    record("offline_delivery_after_restart", [x["text"] for x in received] == expected and all(before <= x["timestamp"] <= before+5 for x in received))
    record("offline_preserves_chat_and_queued_flags", all(x["klass"] == 9 for x in received))
    b.close(); b = Client(RECIPIENT)
    record("no_duplicates_on_relogin", not b.messages_until_pong()); b.close()
    a = Client(SENDER); burst = [f"Kolejka-{i:03d}" for i in range(205)]
    acks = [a.message(text, i+100) for i,text in enumerate(burst)]; wait_pending(205)
    a.close(); stop(); start(); b = Client(RECIPIENT)
    received = [b.receive_message() for _ in burst]; wait_pending(0)
    record("205_messages_survive_restart_in_order", [x["text"] for x in received] == burst and all(x[0] == 3 for x in acks))
    a = Client(SENDER); a.message("Fragmentowany tekst ąćęłńóśźż", 600, fragmented=True)
    received = b.receive_message()
    record("online_message_preserves_text_and_class", received["text"] == "Fragmentowany tekst ąćęłńóśźż" and received["klass"] == 8)
    b.close(); fmt = bytes.fromhex("020300000001")
    a.message("Pogrubione offline", 601, formatting=fmt); wait_pending(1)
    b = Client(RECIPIENT); received = b.receive_message(); wait_pending(0)
    record("offline_formatting_preserved", received["formatting"] == fmt.hex())
    record("unknown_recipient_rejected", a.message("Absent", 602, recipient=9999999)[0] == 6)
    b.close(); a.message("Pending before synchronization",603); wait_pending(1)
    b = Client(RECIPIENT, contacts=False); b.close()
    record("disconnect_before_sync_keeps_message", count_pending() == 1)
    b = Client(RECIPIENT); record("next_login_delivers_waiting_message", b.receive_message()["seq"] == 603); wait_pending(0)
    # Dynamic contacts and unknown extensions must not terminate an established session.
    a.send(0x0d, struct.pack("<IB",RECIPIENT,3)); a.send(0x0e, struct.pack("<IB",RECIPIENT,3)); a.send(0x999,b"unsupported"); a.send(8)
    record("add_remove_and_unknown_extensions_keep_session", a.until(7)[0] == 7)
    # Export all chunks, reimport from the same account after restart, isolate accounts.
    exported = ("Jan;Kowalski;Test;Kontakt;;Retro;1234567;test@example.invalid;0;\r\n" * 70).encode('cp1250')
    for i, offset in enumerate(range(0,len(exported),2047)):
        a.send(0x16, bytes([0 if i == 0 else 1])+exported[offset:offset+2047]); a.until(0x10)
    a.close(); b.close(); stop(); start(); a=Client(SENDER); b=Client(RECIPIENT)
    a.send(0x16,b"\x02"); imported = b""
    while True:
        _, part = a.until(0x10); imported += part[1:]
        if part[0] == 6: break
    record("contact_export_survives_restart", imported == exported)
    b.send(0x16,b"\x02"); record("contact_lists_are_account_scoped", b.until(0x10)[1] == b"\x06")
    # Replacing a connection must not let old cleanup unregister the replacement.
    b2=Client(RECIPIENT); time.sleep(.2)
    for i in range(12):
        a.message(f"new-session-{i}",800+i)
        assert b2.receive_message()["seq"] == 800+i
    b2.send(8); record("session_takeover_preserves_delivery", b2.until(7)[0] == 7)
    b.close(); b2.close(); a.close(); stop(); start()
    a=Client(SENDER,status=0x14); b=Client(RECIPIENT)
    record("invisible_is_offline_in_initial_presence", watch(b,SENDER) == 1)
    a.send(2,struct.pack("<I",2)); b.until(0x0f)
    a.send(2,struct.pack("<I",0x14)); _,status=b.until(0x0f)
    record("invisible_is_offline_in_presence_updates", status[4] == 1)
    a.close(); b.close(); stop(); start()
    a=Client(SENDER,status=0x8002); b=Client(RECIPIENT)
    record("friends_only_hidden_from_stranger", watch(b,SENDER) == 1)
    a.send(0x0d,struct.pack("<IB",RECIPIENT,3)); _,status=b.until(0x0f)
    record("friends_only_visible_to_declared_friend", status[4] == 2)
    b.send(0x10,struct.pack("<IB",SENDER,4)); b.until(0x11)
    a.message("blocked-message",900); time.sleep(.15)
    record("blocked_sender_not_delivered", not b.messages_until_pong()); wait_pending(0)
    a.close(); b.close(); stop(); start()
    # Recipient connects but has not yet sent its block list.
    b=Client(RECIPIENT,contacts=False); a=Client(SENDER)
    a.message("blocked-before-list",901); wait_pending(1)
    record("online_delivery_also_waits_for_contact_sync", not b.messages_until_pong())
    b.send(0x10,struct.pack("<IB",SENDER,4)); b.until(0x11); time.sleep(.1)
    record("block_list_applies_to_early_online_messages", not b.messages_until_pong()); wait_pending(0)
    b.close()
    with sqlite3.connect(DB) as db:
        db.executemany("INSERT INTO messages(recipient_uin,sender_uin,seq,time,class,message) VALUES(?,?,?,1,9,'limit-fixture')", [(RECIPIENT,SENDER,i) for i in range(1000)])
    record("full_mailbox_rejects_without_false_success", a.message("over-limit",999)[0] == 4 and count_pending() == 1000)
    dbquery("DELETE FROM messages WHERE message='limit-fixture'")
    a.message("self-message",1001,recipient=SENDER)
    record("sending_to_self_does_not_deadlock", a.receive_message()["seq"] == 1001)
    a.close(); stop()
    dbquery("UPDATE users SET password=? WHERE uin=?", (PASSWORD,SENDER))
    refused = False
    try:
        start()
    except RuntimeError:
        refused = PROC.poll() is not None
    finally:
        stop()
    record("legacy_passwords_require_explicit_migration", refused)
    start(migrate=True)
    a=Client(SENDER); a.close(); stop()
    record("explicit_migration_preserves_login_and_clears_plaintext", PASSWORD.encode() not in DB.read_bytes() and all(p.startswith("ggv1:") for (p,) in dbquery("SELECT password FROM users")))
    original_key=(RUN/'password.key').read_bytes(); (RUN/'password.key').write_bytes(os.urandom(32))
    refused=False
    try:
        start()
    except RuntimeError:
        refused=PROC.poll() is not None
    finally:
        stop(); (RUN/'password.key').write_bytes(original_key)
    record("wrong_encryption_key_fails_closed", refused)
    start(); a=Client(SENDER); a.close()
    PROC.terminate(); PROC.wait(timeout=25)
    record("graceful_shutdown_exits_cleanly", PROC.returncode == 0)
    stop()
    LOG.flush(); log=(RUN/'server.log').read_text(errors='replace')
    record("no_password_invite_captcha_or_chat_in_logs", all(secret not in log for secret in (PASSWORD,INVITE,expected[0],"blocked-message","self-message")))
    print(json.dumps({"passed": len(RESULTS), "failed":0, "artifacts":str(RUN)}))

if __name__ == '__main__':
    try:
        main()
    finally:
        stop(); LOG.close()
