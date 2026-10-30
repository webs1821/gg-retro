"""Unauthenticated deployment check, intended to run outside the home LAN."""
import socket, struct, urllib.request

host = 'gadu-gadu.k13.pl'
with urllib.request.urlopen(f'https://{host}/healthz', timeout=15) as response:
    assert response.status == 200
with urllib.request.urlopen(f'http://{host}/appsvc/appmsg4.asp', timeout=15) as response:
    fields = response.read(256).decode().split()
    assert len(fields) == 4 and fields[:2] == ['0', '0']
    address, port = fields[2].rsplit(':', 1)
    assert address in {item[4][0] for item in socket.getaddrinfo(host, 8074, family=socket.AF_INET)}
    assert port == '8074'
with socket.create_connection((host, 8074), timeout=10) as client:
    data = b''
    while len(data) < 12:
        part = client.recv(12-len(data))
        assert part, 'Server closed before sending the GG welcome packet'
        data += part
    assert struct.unpack('<II', data[:8]) == (1,4)
print('Public HTTPS, legacy discovery and GG TCP welcome passed.')
