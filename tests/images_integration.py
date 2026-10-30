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


def main():
    w.start()
    users = [int(w.registration(name + '@example.invalid')[1].split(':')[1]) for name in ['images-a', 'images-b', 'images-c']]
    owner, recipient, outsider = users
    w.SENDER, w.RECIPIENT = owner, recipient
    a, b, c = [Client(uin) for uin in users]
    b.send(0x10, struct.pack('<IB', owner, 3))
    w.record('image capacity advertised from login', b.until(0x11)[1][12] == 255)
    a.send(2, struct.pack('<I', 3))
    w.record('status changes preserve image capability', b.wait(lambda p: p[0] == 0x0f and p[1][4] == 3)[1][12] == 255)
    a.send(2, struct.pack('<I', 0x14))
    w.record('invisible presence does not leak image capability', b.wait(lambda p: p[0] == 0x0f and p[1][4] == 1)[1][12] == 0)
    a.send(2, struct.pack('<I', 2))

    data = bmp(); ident = identity(data)
    assert a.message('Obrazek\u00a0 i tekst ąć', 100, formatting=formatting(data, 7))[0] == 3
    message = b.next_text()
    w.record('rich text image descriptor and Polish text survive delivery', message['attrs'] == formatting(data, 7) and message['text'] == 'Obrazek\u00a0 i tekst ąć')
    a.barrier()
    w.record('sending image metadata does not prefetch image bytes', not any(image(p) for p in a.mail))
    assert a.upload(recipient, data) == 6
    w.record('an offer alone does not authorize an unsolicited upload', True)
    assert b.option(owner, 4, ident) == 2
    request = a.next_image(4)
    w.record('only recipient request triggers image fetch from live sender', request['sender'] == recipient and request['id'] == ident)
    assert a.upload(recipient, data, 'zażółć.bmp'.encode('cp1250')) == 2
    w.record('multipart binary image and CP1250 filename delivered intact', b.download(owner, ident) == ('zażółć.bmp'.encode('cp1250'), data))
    b.barrier()
    w.record('image control packets do not pollute the text queue', w.dbquery('SELECT count(*) FROM messages')[0][0] == 1)
    w.record('database contains only image identifiers', {row[1] for row in w.dbquery('PRAGMA table_info(message_images)')} == {'message_id', 'size', 'crc32'})

    assert c.option(owner, 4, ident) == 6
    w.record('known CRC does not give another account access', c.download(owner, ident) is None)
    assert c.option(recipient, 5, ident, b'forged.bmp\0' + data[:50]) == 6
    w.record('unsolicited upload by another account rejected', True)
    assert c.message('\u00a0', 110, formatting=formatting(data))[0] == 3
    assert b.next_text()['sender'] == outsider
    assert b.option(outsider, 4, ident) == 2; c.next_image(4)
    assert c.option(recipient, 5, ident) == 6
    w.record('same CRC offered by another sender cannot reuse previous contents', b.download(outsider, ident) is None)
    b.send(0x0d, struct.pack('<IB', owner, 4)); b.until(0x11)
    assert b.option(owner, 4, ident) == 1
    w.record('recipient block prevents image requests', b.download(owner, ident) is None)
    b.send(0x0e, struct.pack('<IB', owner, 4)); b.until(0x11)
    assert b.option(owner, 4, ident) == 2; a.next_image(4)
    assert a.upload(recipient, data) == 2
    w.record('each new download needs a fresh sender upload', b.download(owner, ident)[1] == data)

    b.close(); b = Client(recipient, image_size=1)
    assert b.option(owner, 4, ident) == 6
    w.record('receiver advertised size limit enforced', b.download(owner, ident) is None)
    b.close()
    offline = PNG; off_id = identity(offline)
    assert a.message('\u00a0', 101, formatting=formatting(offline))[0] == 3
    time.sleep(2.2); a.barrier()
    w.record('offline image message does not trigger a background upload', not any(image(p) for p in a.mail))
    a.close(); c.close(); w.stop(); w.start()
    b = Client(recipient, contacts=False); b.barrier()
    w.record('offline metadata waits for contact synchronization', not any(p[0] == 0x0a for p in b.mail))
    b.send(0x12)
    w.record('offline text and image descriptor survive restart', b.next_text()['attrs'] == formatting(offline))
    assert b.option(owner, 4, off_id) == 6
    w.record('first download fails cleanly while sender is offline', b.download(owner, off_id) is None)
    a = Client(owner); time.sleep(2.2); a.barrier()
    w.record('sender relogin does not automatically fetch pending images', not any(image(p) for p in a.mail))
    assert b.option(owner, 4, off_id) == 2; a.next_image(4)
    assert a.upload(recipient, offline, b'..\\..\\private\\photo.png') == 2
    w.record('recipient can retry after sender returns and paths are stripped', b.download(owner, off_id) == (b'photo.png', offline))
    a.close(); time.sleep(.1)
    assert b.option(owner, 4, off_id) == 6
    w.record('previous successful delivery leaves no server copy for an offline sender', b.download(owner, off_id) is None)
    a = Client(owner)

    bad_data = PNG + b'bad-crc-test'; bad_id = identity(bad_data)
    assert a.message('\u00a0', 102, formatting=formatting(bad_data))[0] == 3; b.next_text()
    assert b.option(owner, 4, bad_id) == 2; a.next_image(4)
    assert a.option(recipient, 5, bad_id, b'bad.png\0' + bytes([bad_data[0] ^ 1]) + bad_data[1:]) == 6
    w.record('CRC mismatch is not forwarded', b.download(owner, bad_id) is None)
    assert b.option(owner, 4, bad_id) == 2; a.next_image(4)
    assert a.option(recipient, 6, bad_id, bad_data[:5]) == 6
    w.record('continuation without first fragment rejected', b.download(owner, bad_id) is None)
    assert b.option(owner, 4, bad_id) == 2; a.next_image(4)
    assert a.option(recipient, 5, bad_id) == 6
    w.record('missing image response terminates download cleanly', b.download(owner, bad_id) is None)
    assert b.option(owner, 4, bad_id) == 2; a.next_image(4)
    assert a.option(recipient, 5, bad_id, b'partial.png\0' + bad_data[:5]) == 2
    a.close()
    w.record('sender disconnect cancels incomplete relay', b.download(owner, bad_id) is None)
    a = Client(owner)
    assert b.option(owner, 4, bad_id) == 2; a.next_image(4)
    assert a.option(recipient, 5, bad_id, b'partial.png\0' + bad_data[:5]) == 2
    b.close(); time.sleep(.1)
    assert a.option(recipient, 6, bad_id, bad_data[5:]) == 6
    w.record('recipient disconnect discards partial image buffers', True)
    b = Client(recipient)
    assert b.option(owner, 4, bad_id) == 2; a.next_image(4)
    assert a.upload(recipient, bad_data) == 2
    w.record('retry starts with a clean assembler after reconnection', b.download(owner, bad_id)[1] == bad_data)

    b.option(owner, 4, off_id, klass=36); a.next_image(4)
    assert a.upload(recipient, offline) == 2
    assert b.download(owner, off_id)[1] == offline
    b.barrier()
    w.record('no-ack flag honored for image control messages', not any(p[0] == 5 for p in b.mail))

    group = [PNG + ('group-' + str(n)).encode() for n in range(4)]
    attrs = b''.join(struct.pack('<HB', n, 0x80) + b'\x09\x01' + struct.pack('<II', *identity(d)) for n, d in enumerate(group))
    assert a.message('\u00a0' * 4, 105, formatting=b'\x02' + struct.pack('<H', len(attrs)) + attrs)[0] == 3
    assert b.next_text()['attrs'] == b'\x02' + struct.pack('<H', len(attrs)) + attrs
    for d in group:
        assert b.option(owner, 4, identity(d)) == 2
    requests = [a.next_image(4) for _ in range(4)]
    assert {r['id'] for r in requests} == {identity(d) for d in group}
    for d in reversed(group):
        assert a.upload(recipient, d) == 2
        assert b.download(owner, identity(d))[1] == d
    w.record('four requested images remain separate when uploads finish out of order', True)
    too_many = attrs + struct.pack('<HB', 4, 0x80) + b'\x09\x01' + struct.pack('<II', 100, 123456)
    count = w.dbquery('SELECT count(*) FROM messages')[0][0]
    assert a.message('\u00a0' * 5, 106, formatting=b'\x02' + struct.pack('<H', len(too_many)) + too_many)[0] == 4
    w.record('image count limit rejects the entire message atomically', w.dbquery('SELECT count(*) FROM messages')[0][0] == count)

    largest = PNG + bytes(255 * 1024 - len(PNG))
    assert a.message('\u00a0', 107, formatting=formatting(largest))[0] == 3; b.next_text()
    assert b.option(owner, 4, identity(largest)) == 2; a.next_image(4)
    assert a.upload(recipient, largest) == 2
    w.record('maximum 255 KiB image crosses real TCP in bounded fragments', b.download(owner, identity(largest))[1] == largest)

    mid = PNG + b'mid-transfer-block'; mid_id = identity(mid)
    a.message('\u00a0', 108, formatting=formatting(mid)); b.next_text()
    assert b.option(owner, 4, mid_id) == 2; a.next_image(4)
    b.send(0x0d, struct.pack('<IB', owner, 4)); b.until(0x11)
    assert a.option(recipient, 5, mid_id, b'mid.png\0' + mid) == 1
    w.record('blocking during upload cancels it without leaking the image', b.download(owner, mid_id) is None)
    b.send(0x0e, struct.pack('<IB', owner, 4)); b.until(0x11)

    c = Client(outsider)
    with sqlite3.connect(w.DB) as db:
        used = db.execute('SELECT count(*) FROM message_images i JOIN messages m ON m.id=i.message_id WHERE m.recipient_uin=?', (recipient,)).fetchone()[0]
        filler = db.execute("INSERT INTO messages(recipient_uin,sender_uin,seq,time,class,message,delivered_at) VALUES (?,0,999,0,8,'quota fixture',CURRENT_TIMESTAMP)", (recipient,)).lastrowid
        for number in range(512 - used - 1):
            db.execute('INSERT INTO message_images(message_id,size,crc32) VALUES (?,1,?)', (filler, number))
    before = w.dbquery('SELECT count(*) FROM messages')[0][0]
    with concurrent.futures.ThreadPoolExecutor(max_workers=2) as executor:
        futures = [executor.submit(client.message, '\u00a0', 109 + i, formatting=formatting(PNG)) for i, client in enumerate([a, c])]
        statuses = sorted(f.result()[0] for f in futures)
    w.record('concurrent grants cannot exceed recipient metadata budget', statuses == [3, 4] and w.dbquery('SELECT count(*) FROM messages')[0][0] == before + 1)
    b.next_text(); c.close()
    a.message('Tekst nadal działa', 103)
    w.record('text delivery unaffected by image exchanges', b.next_text()['text'] == 'Tekst nadal działa')
    a.close(); b.close()
    with sqlite3.connect(w.DB) as db:
        db.execute('PRAGMA foreign_keys=ON')
        db.execute('DELETE FROM messages WHERE id=?', (filler,))
    w.record('deleting a message revokes its image grants', not w.dbquery('SELECT 1 FROM message_images WHERE message_id=?', (filler,)))
    w.record('image database integrity', w.dbquery('PRAGMA integrity_check')[0][0] == 'ok')
    w.stop()
    database_bytes = b''.join(p.read_bytes() for p in w.RUN.glob('*.sqlite*'))
    w.record('image contents and filenames absent from database and journal', PNG not in database_bytes and data not in database_bytes and b'photo.png' not in database_bytes)
    log = (w.RUN / 'server.log').read_bytes()
    w.record('image content and filename absent from logs', b'photo.png' not in log and b'bad-crc-test' not in log and PNG not in log)


if __name__ == '__main__':
    try:
        main()
        print('{} image integration checks passed.'.format(len(w.RESULTS)))
    finally:
        w.stop()
