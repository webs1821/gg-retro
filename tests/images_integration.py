"""Real GG6 TCP image exchanges with synthetic accounts and an isolated SQLite database."""
import base64
import concurrent.futures
import socket
import sqlite3
import struct
import time
import zlib
import wire_integration as w

PNG = base64.b64decode('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jZ5kAAAAASUVORK5CYII=')


def bmp():
    pixels = bytes((i * 17) % 256 for i in range(120 * 100 * 3))
    return b'BM' + struct.pack('<IHHI', 54 + len(pixels), 0, 0, 54) + struct.pack('<IiiHHIIiiII', 40, 120, 100, 1, 24, 0, len(pixels), 2835, 2835, 0, 0) + pixels


def identity(data):
    return len(data), zlib.crc32(data)


def formatting(data, position=0):
    size, crc = identity(data)
    return b'\x02\x0d\x00' + struct.pack('<HB', position, 0x80) + b'\x09\x01' + struct.pack('<II', size, crc)


def image(packet):
    kind, payload = packet
    if kind != 0x0a:
        return None
    sender, seq, timestamp, klass = struct.unpack('<IIII', payload[:16])
    text, options = payload[16:].split(b'\0', 1)
    if text or not options or options[0] not in (4, 5, 6):
        return None
    flag, size, crc = struct.unpack('<BII', options[:9])
    return dict(sender=sender, flag=flag, id=(size, crc), body=options[9:], klass=klass)


class Client(w.Client):
    def __init__(self, *args, **kwargs):
        self.mail = []
        super().__init__(*args, **kwargs)

    def wait(self, predicate):
        for i, packet in enumerate(self.mail):
            if predicate(packet):
                return self.mail.pop(i)
        for _ in range(2048):
            packet = super().recv()
            if predicate(packet):
                return packet
            self.mail.append(packet)
        raise AssertionError('Expected packet was not received')

    def until(self, kind):
        return self.wait(lambda p: p[0] == kind)

    def next_image(self, flag=None, sender=None):
        return image(self.wait(lambda p: image(p) is not None and (flag is None or image(p)['flag'] == flag) and (sender is None or image(p)['sender'] == sender)))

    def next_text(self):
        packet = self.wait(lambda p: p[0] == 0x0a and image(p) is None)
        sender, seq, timestamp, klass = struct.unpack('<IIII', packet[1][:16])
        text, attrs = packet[1][16:].split(b'\0', 1)
        return dict(sender=sender, seq=seq, klass=klass, text=text.decode('cp1250'), attrs=attrs)

    def option(self, peer, flag, ident, tail=b'', klass=4):
        self.send(0x0b, struct.pack('<III', peer, 0, klass) + b'\0' + struct.pack('<BII', flag, *ident) + tail)
        if klass & 32:
            return None
        return struct.unpack('<III', self.until(5)[1])[0]

    def upload(self, peer, data, filename=b'image.png', ident=None):
        ident = ident or identity(data)
        offset = 0
        while offset < len(data):
            prefix = filename + b'\0' if offset == 0 else b''
            end = min(len(data), offset + 1900 - len(prefix))
            ack = self.option(peer, 5 if offset == 0 else 6, ident, prefix + data[offset:end])
            if ack != 2:
                return ack
            offset = end
        return ack

    def download(self, owner, ident):
        first = self.next_image(5, owner)
        assert first['id'] == ident and first['klass'] == 4
        if not first['body']:
            return None
        filename, data = first['body'].split(b'\0', 1)
        while len(data) < ident[0]:
            chunk = self.next_image(6, owner)
            assert chunk['id'] == ident
            data += chunk['body']
        assert len(data) == ident[0] and zlib.crc32(data) == ident[1]
        return filename, data

    def barrier(self):
        self.send(8)
        self.until(7)


def stored(owner, recipient, ident):
    return w.dbquery('SELECT i.data FROM message_images i JOIN messages m ON m.id=i.message_id WHERE m.sender_uin=? AND m.recipient_uin=? AND i.size=? AND i.crc32=? ORDER BY m.id DESC LIMIT 1', (owner, recipient, *ident))[0][0]


def main():
    w.start()
    users = [int(w.registration(name + '@example.invalid')[1].split(':')[1]) for name in ['images-a', 'images-b', 'images-c']]
    owner, recipient, outsider = users
    w.SENDER, w.RECIPIENT = owner, recipient
    a, b, c = [Client(uin) for uin in users]
    b.send(0x10, struct.pack('<IB', owner, 3))
    status = b.until(0x11)[1]
    w.record('image capacity advertised from login', status[12] == 255)
    a.send(2, struct.pack('<I', 3))
    status = b.wait(lambda p: p[0] == 0x0f and p[1][4] == 3)[1]
    w.record('status changes preserve image capability', status[12] == 255)
    a.send(2, struct.pack('<I', 0x14))
    status = b.wait(lambda p: p[0] == 0x0f and p[1][4] == 1)[1]
    w.record('invisible presence does not leak image capability', status[12] == 0)
    a.send(2, struct.pack('<I', 2))

    data = bmp()
    ident = identity(data)
    assert a.message('Obrazek\u00a0 i tekst ąć', 100, formatting=formatting(data, 7))[0] == 3
    message = b.next_text()
    w.record('rich text image descriptor and Polish text survive delivery', message['attrs'] == formatting(data, 7) and message['text'] == 'Obrazek\u00a0 i tekst ąć')
    request = a.next_image(4)
    w.record('server prefetches only the offered image for its recipient', request['sender'] == recipient and request['id'] == ident)
    assert b.option(owner, 4, ident) == 2  # Join the upload already being prefetched.
    assert a.upload(recipient, data, 'zażółć.bmp'.encode('cp1250')) == 2
    w.record('verified image stored before final upload acknowledgement', stored(owner, recipient, ident) == data)
    result = b.download(owner, ident)
    w.record('multipart binary image and CP1250 filename delivered intact', result == ('zażółć.bmp'.encode('cp1250'), data))
    b.barrier()
    w.record('image control packets do not pollute the text queue', w.dbquery('SELECT count(*) FROM messages')[0][0] == 1)

    assert c.option(owner, 4, ident) == 6
    w.record('known CRC does not give another account access', c.download(owner, ident) is None)
    assert c.option(recipient, 5, ident, b'forged.bmp\0' + data[:50]) == 6
    w.record('unsolicited image upload rejected', stored(owner, recipient, ident) == data)
    assert c.message('\u00a0', 110, formatting=formatting(data))[0] == 3
    assert b.next_text()['sender'] == outsider; c.next_image(4)
    assert c.option(recipient, 5, ident) == 6
    assert b.option(outsider, 4, ident) == 2; c.next_image(4)
    assert c.option(recipient, 5, ident) == 6
    w.record('same CRC offered by another sender cannot reuse private content', b.download(outsider, ident) is None)
    b.send(0x0d, struct.pack('<IB', owner, 4)); b.until(0x11)
    assert b.option(owner, 4, ident) == 1
    w.record('recipient block also prevents cached image access', b.download(owner, ident) is None)
    b.send(0x0e, struct.pack('<IB', owner, 4)); b.until(0x11)
    assert b.option(owner, 4, ident) == 2
    w.record('unblocking restores authorized cached access', b.download(owner, ident)[1] == data)

    # Reusing an identifier creates a new private attachment, rather than depending on an old row's lifetime.
    assert a.message('Jeszcze raz', 104, formatting=formatting(data))[0] == 3; b.next_text()
    w.record('repeated image reuses only same-conversation cache', stored(owner, recipient, ident) == data)
    b.close(); b = Client(recipient, image_size=1)
    assert b.option(owner, 4, ident) == 6
    w.record('receiver advertised size limit enforced', b.download(owner, ident) is None)

    b.close()
    offline = PNG
    off_id = identity(offline)
    assert a.message('\u00a0', 101, formatting=formatting(offline))[0] == 3
    assert a.next_image(4)['id'] == off_id
    assert a.upload(recipient, offline, b'..\\..\\private\\photo.png') == 2
    a.close(); c.close(); w.stop()
    restored = w.RUN / 'restored.sqlite'
    with sqlite3.connect(w.DB) as source:
        with sqlite3.connect(restored) as target:
            source.backup(target)
    w.DB = restored
    w.start()
    b = Client(recipient, contacts=False); b.barrier()
    w.record('offline image message waits for contact synchronization', not any(p[0] == 0x0a for p in b.mail))
    b.send(0x12)
    message = b.next_text()
    assert b.option(owner, 4, off_id) == 2
    result = b.download(owner, off_id)
    w.record('offline image survives restart with sender disconnected', message['seq'] == 101 and message['klass'] == 9 and result == (b'photo.png', offline))
    w.record('consistent SQLite backup includes the image contents', stored(owner, recipient, off_id) == offline)
    w.record('image cache never stores a supplied directory path', w.dbquery('SELECT filename FROM message_images WHERE crc32=?', (off_id[1],))[0][0] == b'photo.png')

    a = Client(owner)
    bad_data = PNG + b'bad-crc-test'
    bad_id = identity(bad_data)
    a.message('\u00a0', 102, formatting=formatting(bad_data)); b.next_text(); a.next_image(4)
    assert b.option(owner, 4, bad_id) == 2
    corrupt = bytearray(bad_data); corrupt[-1] ^= 1
    assert a.upload(recipient, bytes(corrupt), ident=bad_id) == 6
    w.record('CRC mismatch is neither cached nor forwarded', b.download(owner, bad_id) is None and stored(owner, recipient, bad_id) is None)
    assert b.option(owner, 4, bad_id) == 2; a.next_image(4)
    assert a.option(recipient, 6, bad_id, b'out-of-order') == 6
    w.record('continuation without first fragment rejected', b.download(owner, bad_id) is None)
    assert b.option(owner, 4, bad_id) == 2; a.next_image(4)
    assert a.option(recipient, 5, bad_id) == 6
    w.record('missing image response terminates download cleanly', b.download(owner, bad_id) is None)
    assert b.option(owner, 4, bad_id) == 2; a.next_image(4)
    assert a.option(recipient, 5, bad_id, b'ok.png\0' + bad_data[:10]) == 2
    a.close()
    w.record('sender disconnect cancels incomplete upload', b.download(owner, bad_id) is None and stored(owner, recipient, bad_id) is None)
    a = Client(owner)
    assert b.option(owner, 4, bad_id) == 2; a.next_image(4)
    assert a.upload(recipient, bad_data) == 2
    w.record('retry starts with a clean assembler after reconnection', b.download(owner, bad_id)[1] == bad_data)

    b.option(owner, 4, off_id, klass=36)
    assert b.download(owner, off_id)[1] == offline
    b.barrier()
    w.record('no-ack flag honored for image control messages', not any(p[0] == 5 for p in b.mail))

    group = [PNG + ('group-' + str(n)).encode() for n in range(4)]
    attrs = b''.join(struct.pack('<HB', n, 0x80) + b'\x09\x01' + struct.pack('<II', *identity(d)) for n, d in enumerate(group))
    assert a.message('\u00a0' * 4, 105, formatting=b'\x02' + struct.pack('<H', len(attrs)) + attrs)[0] == 3
    assert b.next_text()['attrs'] == b'\x02' + struct.pack('<H', len(attrs)) + attrs
    requests = [a.next_image(4) for _ in range(4)]
    assert {r['id'] for r in requests} == {identity(d) for d in group}
    for d in reversed(group):
        assert a.upload(recipient, d) == 2
        assert b.option(owner, 4, identity(d)) == 2
        assert b.download(owner, identity(d))[1] == d
    w.record('four images in one message remain separate when uploads finish out of order', True)
    too_many = attrs + struct.pack('<HB', 4, 0x80) + b'\x09\x01' + struct.pack('<II', 100, 123456)
    count = w.dbquery('SELECT count(*) FROM messages')[0][0]
    assert a.message('\u00a0' * 5, 106, formatting=b'\x02' + struct.pack('<H', len(too_many)) + too_many)[0] == 4
    w.record('image count limit rejects the entire message atomically', w.dbquery('SELECT count(*) FROM messages')[0][0] == count)

    largest = PNG + bytes(255 * 1024 - len(PNG))
    assert a.message('\u00a0', 107, formatting=formatting(largest))[0] == 3; b.next_text(); a.next_image(4)
    assert a.upload(recipient, largest) == 2
    assert b.option(owner, 4, identity(largest)) == 2
    w.record('maximum 255 KiB image crosses real TCP in bounded fragments', b.download(owner, identity(largest))[1] == largest)

    mid = PNG + b'mid-transfer-block'
    mid_id = identity(mid)
    a.message('\u00a0', 108, formatting=formatting(mid)); b.next_text(); a.next_image(4)
    assert b.option(owner, 4, mid_id) == 2
    b.send(0x0d, struct.pack('<IB', owner, 4)); b.until(0x11)
    assert a.option(recipient, 5, mid_id, b'mid.png\0' + mid) == 1
    w.record('blocking during upload cancels it without leaking the image', b.download(owner, mid_id) is None and stored(owner, recipient, mid_id) is None)
    b.send(0x0e, struct.pack('<IB', owner, 4)); b.until(0x11)

    # Fill only a synthetic receiver's reservation budget, then race two accepted senders.
    c = Client(outsider)
    with sqlite3.connect(w.DB) as db:
        used = db.execute('SELECT coalesce(sum(i.size),0) FROM message_images i JOIN messages m ON m.id=i.message_id WHERE m.recipient_uin=?', (recipient,)).fetchone()[0]
        filler = db.execute("INSERT INTO messages(recipient_uin,sender_uin,seq,time,class,message,delivered_at) VALUES (?,0,999,0,8,'quota fixture',CURRENT_TIMESTAMP)", (recipient,)).lastrowid
        remaining = 16 * 1024 * 1024 - used - 1500
        number = 1
        while remaining:
            size = min(255 * 1024, remaining)
            db.execute('INSERT INTO message_images(message_id,size,crc32) VALUES (?,?,?)', (filler, size, number))
            remaining -= size; number += 1
    before = w.dbquery('SELECT count(*) FROM messages')[0][0]
    race_data = PNG + bytes(1000 - len(PNG))
    with concurrent.futures.ThreadPoolExecutor(max_workers=2) as executor:
        futures = [executor.submit(client.message, '\u00a0', 109 + i, formatting=formatting(race_data)) for i, client in enumerate([a, c])]
        statuses = sorted(f.result()[0] for f in futures)
    w.record('concurrent reservations cannot exceed the recipient budget', statuses == [3, 4] and w.dbquery('SELECT count(*) FROM messages')[0][0] == before + 1)
    b.next_text()  # The single accepted raced message.
    c.close()

    # Text/registration remain usable after malformed clients; no production endpoints or codes.
    a.message('Tekst nadal działa', 103)
    w.record('text delivery unaffected by image exchanges', b.next_text()['text'] == 'Tekst nadal działa')
    a.close(); b.close()
    with sqlite3.connect(w.DB) as db:
        db.execute('PRAGMA foreign_keys=ON')
        db.execute('DELETE FROM messages WHERE id=?', (filler,))
        original = db.execute('SELECT min(id) FROM messages WHERE seq=104').fetchone()[0]
        db.execute('DELETE FROM messages WHERE id=?', (original,))
    w.record('deleting a message also deletes its private attachments', not w.dbquery('SELECT 1 FROM message_images WHERE message_id IN (?,?)', (filler, original)))
    w.record('image database integrity', w.dbquery('PRAGMA integrity_check')[0][0] == 'ok')
    w.stop()
    log = (w.RUN / 'server.log').read_bytes()
    w.record('image content and filename absent from logs', b'photo.png' not in log and b'bad-crc-test' not in log and PNG not in log)


if __name__ == '__main__':
    try:
        main()
        print('{} image integration checks passed.'.format(len(w.RESULTS)))
    finally:
        w.stop()
