"""Download HTTP behavior and startup validation; isolated temporary database."""
import hashlib
import os
import urllib.error
import urllib.request
import wire_integration as w

directory = w.RUN / 'downloads'
directory.mkdir()
client = b'MZsynthetic-client'
installer = b'MZsynthetic-installer'
(directory / 'gg.exe').write_bytes(client)
(directory / 'gg-setup.exe').write_bytes(installer)
client_hash = hashlib.sha256(client).hexdigest()
installer_hash = hashlib.sha256(installer).hexdigest()


def manifest(host='127.0.0.1', digest=client_hash, setup=True):
    text = 'hostname = "{}"\ngg_exe_sha256 = "{}"\n'.format(host, digest)
    if setup:
        text += 'setup_exe_sha256 = "{}"\n'.format(installer_hash)
    (directory / 'manifest.toml').write_text(text)


def get(path):
    try:
        with urllib.request.urlopen('http://127.0.0.1:{}{}'.format(w.HTTP_PORT, path), timeout=5) as response:
            return response.status, response.headers, response.read()
    except urllib.error.HTTPError as error:
        return error.code, error.headers, error.read()


def rejects(name):
    rejected = False
    try:
        w.start()
    except RuntimeError:
        rejected = True
    finally:
        w.stop()
    w.record(name, rejected)


try:
    os.environ['GG_CLIENT_DOWNLOADS_DIR'] = str(directory)
    manifest()
    w.start()
    status, headers, body = get('/downloads/gg.exe')
    w.record('client bytes and attachment headers', status == 200 and body == client
             and headers['Content-Type'] == 'application/octet-stream'
             and headers['X-Content-Type-Options'] == 'nosniff'
             and headers['Content-Disposition'] == 'attachment; filename=gg.exe')
    status, headers, body = get('/downloads/gg-setup.exe')
    w.record('installer bytes and attachment', status == 200 and body == installer
             and headers['Content-Disposition'].startswith('attachment;'))
    html = get('/')[2].decode()
    w.record('configured links, digests and replacement instructions',
             '/downloads/gg.exe' in html and '/downloads/gg-setup.exe' in html
             and client_hash in html and installer_hash in html
             and 'Program Files' in html and 'dobreprogramy' not in html.lower()
             and 'update.tmp' in html and 'domyślne dźwięki' in html
             and '/static/patcher.js?v=' in html)
    w.record('fixed routes cannot expose manifest or traverse files',
             get('/downloads/manifest.toml')[0] == 404
             and get('/downloads/../password.key')[0] == 404)
    w.stop()
    manifest(setup=False)
    w.start()
    w.record('installer optional, even if file remains on disk',
             get('/downloads/gg-setup.exe')[0] == 404 and '/downloads/gg-setup.exe' not in get('/')[2].decode()
             and get('/downloads/gg.exe')[0] == 200)
    w.stop()
    manifest(host='different.test')
    rejects('reject wrong hostname')
    manifest(digest='0' * 64)
    rejects('reject wrong checksum')
    manifest()
    (directory / 'gg.exe').unlink()
    rejects('reject missing configured file')
    os.environ['GG_CLIENT_DOWNLOADS_DIR'] = ''
    w.start()
    html = get('/')[2].decode()
    w.record('disabled downloads preserve original installer and patcher',
             get('/downloads/gg.exe')[0] == 404 and get('/downloads/gg-setup.exe')[0] == 404
             and '/downloads/gg.exe' not in html and '/downloads/gg-setup.exe' not in html
             and '/static/gg61.exe' in html and 'patcher' in html)
finally:
    w.stop()
