"""GG6 directory over real TCP, synthetic accounts only; no production data."""
import json
import socket
import struct
import time
import images_integration as images
import wire_integration as w

SEQ = 100
def query(client, kind=3, fields=None, fragmented=False, sequence=None):
    global SEQ
    SEQ += 1
    seq=SEQ if sequence is None else sequence
    body=struct.pack('<BI',kind,seq)
    for key,value in (fields or {}).items():body+=key.encode('ascii')+b'\0'+str(value).encode('cp1250')+b'\0'
    client.send(0x14,body,fragmented)
    data=client.until(0x0e)[1]
    # Native GG 6.1.0.158: READ result is 3; 2 means failed WRITE, not READ.
    assert data[0]=={1:1,2:3,3:5}[kind] and struct.unpack('<I',data[1:5])[0]==seq
    rows=[]
    # Empty values make split-on-double-NUL ambiguous; parse field/value pairs.
    offset=5;current={}
    while offset<len(data):
        if data[offset]==0:
            rows.append(current);current={};offset+=1
        end=data.index(0,offset);key=data[offset:end].decode('ascii').lower();offset=end+1
        end=data.index(0,offset);value=data[offset:end].decode('cp1250');offset=end+1
        assert key not in current
        current[key]=value
    if current:rows.append(current)
    nextstart=int(rows.pop()['nextstart']) if rows and 'nextstart' in rows[-1] else 0
    if kind==3:
        # Native GG6's scanner at 0x416d7a treats any double NUL as a new
        # person, including empty values. Compare its record count with the
        # pair-aware protocol parser above, and exclude the pagination footer.
        native_rows=[part for part in data[5:].split(b'\0\0') if part and not part.lower().startswith(b'nextstart\0')]
        assert len(native_rows)==len(rows),'native GG6 sees extra search rows'
    return rows,nextstart

def rejected(client, kind, fields):
    global SEQ
    SEQ+=1;body=struct.pack('<BI',kind,SEQ)
    for k,v in fields.items():body+=k.encode()+b'\0'+str(v).encode('cp1250')+b'\0'
    client.send(0x14,body)
    try:
        while True:
            packet=client.recv()
            assert packet[0]!=0x0e,'Invalid write was acknowledged'
    except (EOFError,ConnectionResetError):pass
    finally:client.close()

def main():
    w.start()
    with socket.create_connection(('127.0.0.1',w.PORT),timeout=5) as unauth:
        unauth.recv(12);unauth.sendall(struct.pack('<IIBI',0x14,5,2,77))
        w.record('directory requires an authenticated session',unauth.recv(1)==b'')
    owner,viewer,outsider=[int(w.registration(n+'@example.invalid')[1].split(':')[1]) for n in ['directory-owner','directory-viewer','directory-outsider']]
    a,b,c=[images.Client(u) for u in [owner,viewer,outsider]]
    for x in [a,b,c]:x.barrier()
    w.record('new accounts are not automatically public',query(b)[0]==[] and query(b,fields={'FmNumber':owner})[0]==[])
    own=query(a,2)[0][0]
    w.record('own initially empty profile never exposes registration name or email',own['fmnumber']==str(owner) and own['firstname']=='' and 'email' not in own)
    for seq in [0,0xffffffff]:
        w.record('native GG6 READ accepts arbitrary sequence '+str(seq),query(a,2,sequence=seq)[0][0]['firstname']=='')
    query(a,1,{'firstname':'Jan','nickname':'Retro','birthyear':'1987'})
    sparse=query(b,fields={'birthyear':'1980 1990'})[0]
    w.record('native GG6 age search shows one person when city is blank',len(sparse)==1 and sparse[0]['fmnumber']==str(owner) and 'city' not in sparse[0])
    query(a,1,{'nickname':'Only nick'})
    sparse=query(b,fields={'fmnumber':owner})[0]
    w.record('unknown age and blank text are omitted from search results',len(sparse)==1 and set(sparse[0])=={'fmnumber','nickname','fmstatus'})
    fields={'firstname':'Łucja','lastname':'Kowalska','nickname':'Żółwik','city':'Łódź','familyname':'Nowak','familycity':'Gdańsk','birthyear':'1984','gender':'2'}
    w.record('fragmented CP1250 write acknowledged with request sequence and own UIN',query(a,1,fields,True)[0]==[{'fmnumber':str(owner)}])
    own=query(a,2)[0][0];w.record('full own profile roundtrips',all(own[k]==v for k,v in fields.items()))
    found=query(b,fields={'FmNumber':owner})[0]
    w.record('exact UIN lookup returns public fields and visible status',len(found)==1 and found[0]['firstname']=='Łucja' and found[0]['fmstatus']=='2')
    w.record('search does not expose surname gender family details email or credentials',set(found[0])=={'fmnumber','firstname','nickname','city','birthyear','fmstatus'})
    w.record('Polish and ASCII case-insensitive partial search',len(query(b,fields={'firstname':'LUC','nickname':'zolw','city':'LODZ'})[0])==1)
    w.record('maiden name and former city participate in matching',len(query(b,fields={'lastname':'now','city':'gdansk'})[0])==1)
    w.record('gender search reverses own-profile mapping',len(query(b,fields={'gender':'1','birthyear':'1980 1990'})[0])==1 and query(b,fields={'gender':'2'})[0]==[])
    w.record('unmatched year and combined filters return no results',query(b,fields={'birthyear':'1990 2000'})[0]==[] and query(b,fields={'firstname':'Łucja','city':'Warszawa'})[0]==[])
    w.record('SQL injection and wildcard characters are literal',query(b,fields={'nickname':"%' OR 1=1--"})[0]==[])
    w.record('unsupported email search cannot broaden results',query(b,fields={'email':'directory-owner@example.invalid'})[0]==[])
    w.record('active-only includes visible available user',len(query(b,fields={'ActiveOnly':'1'})[0])==1)
    for status in [3,4,5]:
        a.send(2,struct.pack('<I',status)+(b'Busy' if status in (4,5) else b''));a.barrier()
        w.record('active-only includes GG status '+str(status),query(b,fields={'ActiveOnly':'1'})[0][0]['fmstatus']==str(status))
    a.send(2,struct.pack('<I',0x14));a.barrier()
    w.record('invisible status remains offline and absent from active-only',query(b,fields={'ActiveOnly':'1'})[0]==[] and query(b,fields={'fmnumber':owner})[0][0]['fmstatus']=='1')
    a.send(0x10,struct.pack('<IB',viewer,3));a.until(0x11)
    a.send(2,struct.pack('<I',0x8002));a.barrier()
    w.record('friends-only presence is visible to friend',len(query(b,fields={'ActiveOnly':'1'})[0])==1)
    w.record('friends-only presence is hidden from stranger',query(c,fields={'ActiveOnly':'1'})[0]==[] and query(c,fields={'fmnumber':owner})[0][0]['fmstatus']=='1')
    a.send(0x10,struct.pack('<IB',viewer,4));a.until(0x11);a.send(2,struct.pack('<I',2));a.barrier()
    w.record('blocked viewer cannot use directory to reveal online status',query(b,fields={'ActiveOnly':'1'})[0]==[] and query(b,fields={'fmnumber':owner})[0][0]['fmstatus']=='1')
    # Attempts to impersonate another profile must not alter it or acknowledge success.
    before=w.dbquery('SELECT * FROM public_profiles')
    rejected(c,1,{'fmnumber':owner,'nickname':'Stolen'})
    w.record('write ownership comes from authenticated session',w.dbquery('SELECT * FROM public_profiles')==before)
    c=images.Client(outsider);rejected(c,2,{'fmnumber':owner})
    w.record('READ cannot fetch somebody elses private profile fields',True)
    c=images.Client(outsider);rejected(c,1,{'nickname':'X'*51})
    w.record('oversized profile is rejected atomically',w.dbquery('SELECT * FROM public_profiles')==before)
    query(a,1,{'nickname':'Nowy Żółwik'},sequence=0)
    own=query(a,2)[0][0]
    w.record('native full-form WRITE clears omitted fields and accepts zero sequence',own['nickname']=='Nowy Żółwik' and all(own[k]=='' for k in ['firstname','lastname','city','familyname','familycity']) and own['birthyear']=='0' and own['gender']=='0')
    a.close();b.close();w.stop();w.start();a=images.Client(owner);b=images.Client(viewer)
    w.record('profile survives restart',query(a,2)[0][0]['nickname']=='Nowy Żółwik')
    # GG6 omits blank fields, and may retain a selected gender in an empty form.
    query(a,1,{'gender':'2'})
    w.record('clearing profile removes it from public search',query(b,fields={'fmnumber':owner})[0]==[] and w.dbquery('SELECT count(*) FROM public_profiles')==[(0,)])
    # Populate many synthetic offline records directly for pagination, without
    # consuming public invitations or reconnecting hundreds of clients.
    for uin in range(500001,500046):
        w.dbquery('INSERT INTO users(uin,name,email,password) VALUES(?,?,?,?)',(uin,'private name',str(uin)+'@example.invalid','unused-test-only'))
        w.dbquery('INSERT INTO public_profiles(uin,nickname,nick_key) VALUES(?,?,?)',(uin,'Page user','page user'))
    seen=[];cursor=0;pages=[]
    while True:
        rows,cursor=query(b,fields={'nickname':'page','fmstart':cursor});seen.extend(int(r['fmnumber']) for r in rows);pages.append(len(rows))
        if not cursor:break
    w.record('20-row pagination is inclusive stable and has no gaps or duplicates',seen==list(range(500001,500046)) and pages==[20,20,5])
    w.record('active filter excludes synthetic offline profiles',query(b,fields={'nickname':'page','ActiveOnly':'1'})[0]==[])
    w.record('next page cursor beyond end returns empty result',query(b,fields={'fmstart':16777215})==([],0))
    # Explicitly test the session request limit using a fresh valid session.
    c=images.Client(outsider)
    for _ in range(30):query(c,2)
    rejected(c,2,{})
    w.record('directory request rate limit enforced',True)
    # Distinct fresh session: write limit is lower than the general directory limit.
    c=images.Client(outsider)
    for _ in range(10):query(c,1,{'nickname':'Rate limit test'})
    rejected(c,1,{'nickname':'Eleventh write'})
    w.record('profile write rate limit enforced without extra write',w.dbquery('SELECT nickname FROM public_profiles WHERE uin=?',(outsider,))==[('Rate limit test',)])
    a.close();b.close()
    w.record('database is consistent',w.dbquery('PRAGMA integrity_check')==[('ok',)])
    w.LOG.flush();logs=(w.RUN/'server.log').read_bytes()
    w.record('profile data and registration email stay out of logs',all(x not in logs for x in ['Łucja'.encode(),'Nowy Żółwik'.encode(),b'directory-owner@example.invalid',b'Kowalska']))
    print(json.dumps({'passed':len(w.RESULTS),'failed':0}))

if __name__=='__main__':
    try:main()
    finally:w.stop()
