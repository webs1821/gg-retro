"""GG6 conference exchanges through TCP, using only temporary synthetic accounts."""
import struct
import time
import images_integration as images
import wire_integration as w


def conference(others):
    return b'\x01' + struct.pack('<I', len(others)) + b''.join(struct.pack('<I', uin) for uin in others)


def unpack(attrs):
    assert attrs[0] == 1
    count = struct.unpack('<I', attrs[1:5])[0]
    return list(struct.unpack('<' + 'I' * count, attrs[5:5 + count * 4])), attrs[5 + count * 4:]


def main():
    w.start()
    ids = [int(w.registration('conference-' + name + '@example.invalid')[1].split(':')[1]) for name in ['a', 'b', 'c']]
    au, bu, cu = ids
    w.SENDER, w.RECIPIENT = au, bu
    a, b, c = [images.Client(uin) for uin in ids]
    rich = b'\x02\x06\x00\x00\x00\x09\x12\x34\x56'

    assert a.message('Grupa: ąćęłńóśźż', 100, bu, conference([cu]) + rich)[0] == 3
    msg = b.next_text(); others, formatting = unpack(msg['attrs'])
    w.record('conference retains other participants and CP1250 formatting', msg['sender'] == au and others == [cu] and formatting == rich and msg['text'] == 'Grupa: ąćęłńóśźż')
    c.barrier()
    w.record('participant list does not trigger server fanout', not any(p[0] == 10 for p in c.mail))
    assert a.message('Grupa: ąćęłńóśźż', 100, cu, conference([bu]) + rich)[0] == 3
    msg = c.next_text(); others, _ = unpack(msg['attrs'])
    w.record('client fanout creates the same room for each destination', set([msg['sender'], cu] + others) == set(ids))
    assert b.message('Odpowiedź w grupie', 101, au, conference([cu]))[0] == 3
    msg = a.next_text()
    w.record('reply preserves conference identity', msg['sender'] == bu and unpack(msg['attrs'])[0] == [cu])
    assert b.message('Odpowiedź w grupie', 101, cu, conference([au]))[0] == 3
    c.next_text()
    for client in [a, b, c]:
        client.barrier()
        assert not any(p[0] == 10 for p in client.mail)
    w.record('no duplicate messages after client fanout', w.dbquery('SELECT count(*) FROM messages')[0][0] == 4)

    c.close()
    assert a.message('Konferencja offline', 102, cu, conference([bu]) + rich)[0] == 3
    a.close(); b.close(); w.stop(); w.start()
    c = images.Client(cu, contacts=False); c.barrier()
    w.record('offline conference waits for privacy synchronization', not any(p[0] == 10 for p in c.mail))
    c.send(0x12); msg = c.next_text()
    w.record('restart preserves conference participants, text, class and formatting', msg['text'] == 'Konferencja offline' and msg['klass'] & 1 and msg['attrs'] == conference([bu]) + rich)
    c.close(); c = images.Client(cu); c.barrier()
    w.record('relogin does not duplicate an already delivered conference', not any(p[0] == 10 for p in c.mail))
    a, b = images.Client(au), images.Client(bu)

    assert a.message('\u00a0', 103, bu, conference([cu]) + images.formatting(images.PNG))[0] == 3
    assert a.message('\u00a0', 103, cu, conference([bu]) + images.formatting(images.PNG))[0] == 3
    assert b.next_text()['attrs'] == conference([cu]) + images.formatting(images.PNG)
    assert c.next_text()['attrs'] == conference([bu]) + images.formatting(images.PNG)
    a.barrier()
    w.record('conference images keep classic on-demand behavior', not any(images.image(p) for p in a.mail))
    ident = images.identity(images.PNG)
    assert b.option(au, 4, ident) == 2 and c.option(au, 4, ident) == 2
    requests = [a.next_image(4), a.next_image(4)]
    assert {r['sender'] for r in requests} == {bu, cu}
    assert a.upload(bu, images.PNG) == 2 and a.upload(cu, images.PNG) == 2
    w.record('each conference recipient receives its requested image', b.download(au, ident)[1] == images.PNG and c.download(au, ident)[1] == images.PNG)
    a.close(); time.sleep(.1)
    assert c.option(au, 4, ident) == 6
    w.record('conference images still require an online sender', c.download(au, ident) is None)
    a = images.Client(au)

    # Conference metadata alone must not authorize fetching an image sent only to somebody else.
    private = images.PNG + b'private-image'
    assert a.message('\u00a0', 104, bu, conference([cu]) + images.formatting(private))[0] == 3
    b.next_text()
    assert c.option(au, 4, images.identity(private)) == 6
    w.record('listed participant without its own message has no image grant', c.download(au, images.identity(private)) is None)

    b.send(0x0d, struct.pack('<IB', au, 4)); b.until(0x11)
    assert a.message('Blocked group message', 105, bu, conference([cu]))[0] == 3
    b.barrier()
    w.record('conference does not bypass recipient block', not any(p[0] == 10 for p in b.mail))
    assert a.message('Only actual destination', 106, cu, conference([bu]))[0] == 3
    assert c.next_text()['text'] == 'Only actual destination'
    b.barrier()
    w.record('blocked listed member receives no copy through another destination', not any(p[0] == 10 for p in b.mail))
    b.send(0x0e, struct.pack('<IB', au, 4)); b.until(0x11)

    before = w.dbquery('SELECT count(*) FROM messages')[0][0]
    assert a.message('Spoofed sender entry', 107, bu, conference([au]))[0] == 6
    w.record('authenticated sender cannot also appear as another participant', w.dbquery('SELECT count(*) FROM messages')[0][0] == before)
    assert a.message('Order compatibility', 108, bu, rich + conference([cu]))[0] == 3
    w.record('attribute order normalized for GG6 receivers', b.next_text()['attrs'] == conference([cu]) + rich)
    members = list(range(1001, 1019))
    assert a.message('Twenty-person bound', 109, bu, conference(members))[0] == 3
    w.record('twenty-person limit accepts exactly eighteen other participants', unpack(b.next_text()['attrs'])[0] == members)
    a.close()
    for attrs in [conference([bu]), conference([cu, cu]), b'\x01\xff\xff\xff\xff', conference(list(range(1001, 1020)))]:
        malformed = images.Client(au)
        before = w.dbquery('SELECT count(*) FROM messages')[0][0]
        malformed.send(11, struct.pack('<III', bu, 110, 8) + b'bad\0' + attrs)
        try:
            malformed.until(7)
            raise AssertionError('Malformed conference stayed connected')
        except (EOFError, ConnectionResetError):
            pass
        malformed.close()
        assert w.dbquery('SELECT count(*) FROM messages')[0][0] == before
    w.record('malformed or oversized conference packets are rejected before storing', True)
    a = images.Client(au)
    assert a.message('Prywatna rozmowa nadal działa', 111, bu)[0] == 3
    msg = b.next_text()
    w.record('one-to-one chat remains unchanged after malformed groups', msg['text'] == 'Prywatna rozmowa nadal działa' and not msg['attrs'])
    for client in [a, b, c]: client.close()
    w.record('database integrity after conference exchanges', w.dbquery('PRAGMA integrity_check')[0][0] == 'ok')
    w.stop()
    log = (w.RUN / 'server.log').read_bytes()
    w.record('conference message contents are absent from logs', b'Blocked group message' not in log and b'private-image' not in log)


if __name__ == '__main__':
    try:
        main()
        print('{} conference integration checks passed.'.format(len(w.RESULTS)))
    finally:
        w.stop()
