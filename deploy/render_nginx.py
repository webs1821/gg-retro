"""Render nginx from the same TOML configuration as the server/client package (Python 3.11+)."""
import argparse,pathlib,re,tomllib
p=argparse.ArgumentParser(description=__doc__);p.add_argument('--config',type=pathlib.Path,required=True);p.add_argument('--output',type=pathlib.Path,required=True);a=p.parse_args()
config=tomllib.loads(a.config.read_text(encoding='utf-8'));host=config['hostname']
assert re.fullmatch(r'[A-Za-z0-9]+(?:[.-][A-Za-z0-9]+)*',host)
backend=config.get('nginx_backend','127.0.0.1:18074');assert re.fullmatch(r'[A-Za-z0-9.-]+:[0-9]{1,5}',backend)
cert=config.get('tls_certificate_dir','/etc/letsencrypt/live/'+host);assert re.fullmatch(r'/[A-Za-z0-9_./-]+',cert) and '..' not in cert.split('/')
template=pathlib.Path(__file__).with_name('nginx.example.conf').read_text()
for name,value in {'HOSTNAME':host,'BACKEND':backend,'CERT_DIR':cert}.items():template=template.replace('{{'+name+'}}',value)
with a.output.open('x',encoding='utf-8',newline='\n') as out:out.write(template)
