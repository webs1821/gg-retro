"""Real GG6 sessions: reconnect semantics and durable, private offline descriptions."""
import json
import socket
import struct
import sys
import time
import urllib.request
import images_integration as images
import wire_integration as w

NOTE = 'Wrócę wieczorem — ąćęłńóśźż'
RETURN = 1793458800

def closing(client):
    packets = list(client.mail); client.mail.clear()
    client.sock.settimeout(3)
    try:
        while True: packets.append(client.recv())
    except EOFError: pass
    finally: client.close()
    return packets

def status(client, value, text=None, timestamp=None):
    body = struct.pack('<I', value)
    if text is not None:
        body += text.encode('cp1250') + b'\0'
        if timestamp is not None: body += struct.pack('<I', timestamp)
    client.send(2, body)

def watch(client, owner):
    client.send(0x10, struct.pack('<IB', owner, 3))
    packet = client.until(0x11)[1]
    assert len(packet) >= 14 and struct.unpack('<I',packet[:4])[0] == owner
    tail = packet[15:15+packet[14]] if len(packet)>14 else b''
    text, _, suffix = tail.partition(b'\0')
    return packet[4], text.decode('cp1250'), struct.unpack('<I',suffix)[0] if len(suffix)==4 else None

def privacy(client, entries):
    client.send(0x10,b''.join(struct.pack('<IB',*entry) for entry in entries));client.until(0x11)
    client.barrier()

def probe():
    w.start(); owner=int(w.registration('probe@example.invalid')[1].split(':')[1])
    a=images.Client(owner);a.barrier()
    w.PROC.terminate(); packets=closing(a);w.PROC.wait(timeout=25)
    print(json.dumps({'shutdown_packet_types':[kind for kind,_ in packets], 'forced_logout':any(kind==11 for kind,_ in packets)}))

def main():
    w.start()
    owner,friend,stranger=[int(w.registration(name+'@example.invalid')[1].split(':')[1]) for name in ['owner','friend','stranger']]
    w.SENDER,w.RECIPIENT=friend,owner
    a,b,c=[images.Client(uin) for uin in [owner,friend,stranger]]
    a.barrier(); b.barrier(); c.barrier()
    w.PROC.terminate()
    packets=[closing(client) for client in [a,b,c]]; w.PROC.wait(timeout=25)
    w.record('SIGTERM closes every session without forced-logout or authentication-failure packet',all(not any(k in (9,11) for k,_ in rows) for rows in packets) and w.PROC.returncode==0)
    w.stop();w.start();a=images.Client(owner);a.barrier()
    w.record('same credentials reconnect immediately after graceful restart',True)
    a2=images.Client(owner);a2.barrier();packets=closing(a)
    w.record('a second login still explicitly disconnects the replaced session',any(k==11 and not p for k,p in packets))
    a=a2;status(a,1);packets=closing(a)
    w.record('selecting unavailable closes TCP without forced logout',not any(k in (9,11) for k,_ in packets))
    a=images.Client(owner);b=images.Client(friend);c=images.Client(stranger)
    a.barrier();w.record('unavailable then available creates a usable session',True)
    assert watch(b,owner)[0]==2
    status(a,0x15,NOTE,RETURN);closing(a)
    changed=b.wait(lambda p:p[0]==15 and p[1][4]==0x15)[1]
    w.record('offline description and return time reach existing observers',changed[14:]==NOTE.encode('cp1250')+b'\0'+struct.pack('<I',RETURN))
    w.record('late observers see full offline description',watch(c,owner)==(0x15,NOTE,RETURN))
    w.record('explicit offline description is durably stored with privacy',w.dbquery('SELECT description,return_time,length(privacy) FROM offline_presence WHERE uin=?',(owner,))==[(NOTE,RETURN,0)])
    assert b.message('Message while owner offline',501,recipient=owner)[0]==3
    b.close();c.close();w.stop();w.start();b=images.Client(friend);c=images.Client(stranger)
    w.record('description and return time survive abrupt process restart',watch(b,owner)==(0x15,NOTE,RETURN))
    a=images.Client(owner,contacts=False);a.barrier()
    w.record('new login clears stale offline description before publishing presence',not w.dbquery('SELECT uin FROM offline_presence WHERE uin=?',(owner,)) and watch(c,owner)==(1,'',None))
    privacy(a,[(friend,3),(stranger,4)])
    w.record('offline text queue still arrives after contact synchronization',a.next_text()['text']=='Message while owner offline')
    status(a,0x8015,NOTE);closing(a)
    w.record('friends-only offline note is visible to a friend',watch(b,owner)==(0x15,NOTE,None))
    w.record('blocked viewer cannot see offline note or return time',watch(c,owner)==(1,'',None))
    b.close();c.close();w.stop();w.start();b=images.Client(friend);c=images.Client(stranger)
    w.record('privacy survives restart together with the note',watch(b,owner)==(0x15,NOTE,None) and watch(c,owner)==(1,'',None))
    a=images.Client(owner);privacy(a,[(friend,3)]);status(a,0x8015,'Tylko znajomi');closing(a)
    w.record('friends-only excludes an unknown viewer, not only blocked accounts',watch(c,owner)==(1,'',None))
    a=images.Client(owner);privacy(a,[(friend,1)]);status(a,0x15,'Ukryty opis');closing(a)
    w.record('always-offline contact cannot see a public offline description',watch(b,owner)==(1,'',None) and watch(c,owner)==(0x15,'Ukryty opis',None))
    a=images.Client(owner);a.barrier();status(a,1);closing(a)
    w.record('plain unavailable removes an earlier description',watch(c,owner)==(1,'',None) and not w.dbquery('SELECT * FROM offline_presence'))
    a=images.Client(owner);a.barrier();status(a,0x15,'');closing(a)
    w.record('empty offline description is cleared, not persisted',not w.dbquery('SELECT * FROM offline_presence'))
    a=images.Client(owner);a.barrier();status(a,4,'Online only');a.barrier();a.close();time.sleep(.1)
    w.record('unexpected client disconnect never persists an online description',watch(c,owner)==(1,'',None) and not w.dbquery('SELECT * FROM offline_presence'))
    a=images.Client(owner);a.barrier();status(a,0x16,'Invisible only');a.barrier();a.close();time.sleep(.1)
    w.record('invisible description is never exposed or persisted',watch(c,owner)==(1,'',None) and not w.dbquery('SELECT * FROM offline_presence'))
    a=images.Client(owner,contacts=False);status(a,0x15,'No privacy yet');closing(a)
    w.record('offline before privacy synchronization fails closed',watch(c,owner)==(1,'',None) and not w.dbquery('SELECT * FROM offline_presence'))
    a=images.Client(owner);a.barrier();status(a,0x15,'x'*71);closing(a)
    w.record('oversized description is rejected before persistence',not w.dbquery('SELECT * FROM offline_presence'))
    a=images.Client(owner);a.barrier();status(a,0x15,'ą'*70);closing(a)
    w.record('70 CP1250 characters remain intact',watch(c,owner)==(0x15,'ą'*70,None))
    b.close();c.close();w.stop();w.start();a=images.Client(owner,status=0x15,contacts=False)
    # An initial offline state must also wait for the contact list and then close.
    a.send(0x12);closing(a)
    w.record('initial offline login closes after contact synchronization',not w.dbquery('SELECT * FROM offline_presence'))
    b=images.Client(friend);c=images.Client(stranger)
    a=images.Client(owner,status=0x8015,contacts=False,description=NOTE,return_time=RETURN)
    w.record('initial offline note stays hidden until privacy arrives',watch(b,owner)==(1,'',None))
    a.send(0x10,struct.pack('<IB',friend,3)+struct.pack('<IB',stranger,4));closing(a)
    w.record('initial offline login persists note and privacy atomically',watch(b,owner)==(0x15,NOTE,RETURN) and watch(c,owner)==(1,'',None))
    a=images.Client(owner,status=0x8004,contacts=False,description='Private initial status')
    c.barrier()
    w.record('initial online description does not leak before contact synchronization',watch(c,owner)==(1,'',None) and all(b'Private initial status' not in p for _,p in c.mail))
    privacy(a,[(friend,3),(stranger,4)])
    w.record('initial described presence respects the completed privacy list',watch(b,owner)==(4,'Private initial status',None) and watch(c,owner)==(1,'',None))
    a.close();b.close();c.close()
    w.record('database remains consistent',w.dbquery('PRAGMA integrity_check')==[('ok',)])
    w.stop();w.LOG.flush();log=(w.RUN/'server.log').read_text(errors='replace')
    w.record('descriptions and credentials are absent from logs',all(s not in log for s in [NOTE,'No privacy yet','Online only',w.PASSWORD,w.INVITE]))
    print(json.dumps({'passed':len(w.RESULTS),'failed':0}))

if __name__=='__main__':
    try: probe() if '--probe' in sys.argv else main()
    finally: w.stop()
