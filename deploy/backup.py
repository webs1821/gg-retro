#!/usr/bin/env python3
"""Consistent online SQLite backup with its indispensable encryption key; root only."""
import datetime, os, pathlib, shutil, sqlite3

root = pathlib.Path('/opt/gg-retro')
os.umask(0o077)
backups = root / 'backups'
backups.mkdir(mode=0o700, exist_ok=True)
stamp = datetime.datetime.now(datetime.timezone.utc).strftime('%Y%m%dT%H%M%S.%fZ')
temporary = backups / ('.' + stamp)
temporary.mkdir(mode=0o700)
with sqlite3.connect(f'file:{root / "data/gg.db"}?mode=ro', uri=True) as source:
    with sqlite3.connect(temporary / 'gg.sqlite') as target:
        source.backup(target)
        if target.execute('PRAGMA integrity_check').fetchone()[0] != 'ok':
            raise RuntimeError('SQLite backup integrity check failed')
shutil.copytree(root / 'secrets', temporary / 'secrets')
if (root / 'downloads').is_dir():
    shutil.copytree(root / 'downloads', temporary / 'downloads')
for name in ('config.toml', 'compose.yaml', '.env'):
    shutil.copy2(root / name, temporary / name)
final = backups / ('gg-retro-' + stamp)
temporary.rename(final)
# Never traverse symlinks or delete unrelated paths.
old = sorted(p for p in backups.glob('gg-retro-*') if p.is_dir() and not p.is_symlink())
for path in old[:-14]:
    if path.resolve().parent == backups.resolve():
        shutil.rmtree(path)
print(final.name)
