"""Python 3.11+ / Node: patch GG and rebuild the original NSIS-2 solid-LZMA installer.

The original installer is pinned by SHA256. Its executable stub, script/header,
license, resources and every other payload byte are preserved. Only gg.exe is
replaced; the compressed stream length and NSIS CRC32 are recalculated.
No Windows executable is executed by this tool.
The shared browser/CLI patcher also disables the obsolete updater and fixes the
default sound basenames in gg.exe; the rebuilt installer installs that same EXE.
"""
import argparse, hashlib, json, lzma, pathlib, re, struct, subprocess, tempfile, tomllib, zlib

ROOT = pathlib.Path(__file__).resolve().parents[1]
ORIGINAL_INSTALLER_SHA256 = 'bcc8157aa6bface009d8018c308bf3cef8725546b4f826bdbaf6bbeaa953b06f'
ORIGINAL_CLIENT_SHA256 = '74b78af318c52f86dff8a7ee27a7a0b5ea545167aebb594a8a559ec97f2e3a57'
CLIENT_SIZE = 790528
SIGNATURE = b'\xef\xbe\xad\xdeNullsoftInst'
def sha(data): return hashlib.sha256(data).hexdigest()
def unpack(data):
    assert data.count(SIGNATURE) == 1, 'Unsupported NSIS header'
    offset = data.index(SIGNATURE)-4
    flags, *_, header_size, total = struct.unpack_from('<7I',data,offset)
    assert flags == 0 and total == len(data)-offset
    assert zlib.crc32(data[512:-4]) == struct.unpack('<I',data[-4:])[0], 'NSIS CRC mismatch'
    props = data[offset+28:offset+33]
    assert props[0] == 0x5d, 'Unsupported LZMA properties'
    filters = [{'id':lzma.FILTER_LZMA1,'dict_size':int.from_bytes(props[1:],'little'),'lc':3,'lp':0,'pb':2}]
    decoder = lzma.LZMADecompressor(format=lzma.FORMAT_RAW,filters=filters)
    stream = decoder.decompress(data[offset+33:],max_length=16*1024*1024)
    assert decoder.eof and decoder.unused_data == data[-4:]
    assert int.from_bytes(stream[:4],'little') == header_size
    return offset, props, filters, stream
def extract_client(stream):
    matches=[];start=0
    while True:
        start=stream.find(b'MZ',start)
        if start < 0: break
        candidate=stream[start:start+CLIENT_SIZE]
        if len(candidate)==CLIENT_SIZE and sha(candidate)==ORIGINAL_CLIENT_SHA256:matches.append(candidate)
        start+=2
    assert len(matches)==1, 'Expected exactly one original GG 6.1 client'
    return matches[0]
def rebuild(original, patched):
    assert sha(original)==ORIGINAL_INSTALLER_SHA256, 'Unexpected source installer'
    offset, props, filters, stream = unpack(original)
    client=extract_client(stream)
    assert len(patched)==len(client) and patched.startswith(b'MZ')
    changed=stream.replace(client,patched)
    assert len(changed)==len(stream) and changed[:4+int.from_bytes(stream[:4],'little')]==stream[:4+int.from_bytes(stream[:4],'little')]
    compressed=lzma.compress(changed,format=lzma.FORMAT_RAW,filters=filters)
    prefix=bytearray(original[:offset+28]);struct.pack_into('<I',prefix,offset+24,28+5+len(compressed)+4)
    output=bytes(prefix)+props+compressed
    output+=struct.pack('<I',zlib.crc32(output[512:]))
    new_offset,_,_,check=unpack(output)
    assert new_offset==offset and output[:offset]==original[:offset]
    assert check.count(patched)==1 and check.replace(patched,client)==stream, 'Unexpected payload changes'
    return output
def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--config',type=pathlib.Path,required=True)
    parser.add_argument('--output',type=pathlib.Path,required=True)
    parser.add_argument('--patched-exe',type=pathlib.Path,help='Optional supplied client; must exactly match the configured patch')
    args=parser.parse_args()
    host=tomllib.loads(args.config.read_text(encoding='utf-8'))['hostname']
    if not re.fullmatch(r'[A-Za-z0-9]+(?:[.-][A-Za-z0-9]+)*',host) or len(host)>16:parser.error('GG6 requires a hostname of at most 16 ASCII characters')
    output=args.output.resolve();output.mkdir(parents=True,exist_ok=True)
    if any(output.iterdir()):parser.error('Output directory must be empty; existing packages are never overwritten')
    original=(ROOT/'server/static/gg61.exe').read_bytes()
    assert sha(original)==ORIGINAL_INSTALLER_SHA256
    client=extract_client(unpack(original)[3])
    with tempfile.TemporaryDirectory(prefix='.gg-build-',dir=output.parent) as directory:
        temp=pathlib.Path(directory).resolve();assert temp.parent==output.parent
        (temp/'original.exe').write_bytes(client)
        subprocess.run(['node',str(ROOT/'tools/patch_client.cjs'),str(temp/'original.exe'),str(temp/'patched.exe'),host],check=True,capture_output=True)
        patched=(temp/'patched.exe').read_bytes()
    if args.patched_exe:
        supplied=args.patched_exe.read_bytes();assert supplied==patched, 'Supplied gg.exe differs from the expected configured patch';patched=supplied
    installer=rebuild(original,patched)
    (output/'gg.exe').write_bytes(patched);(output/'gg-setup.exe').write_bytes(installer)
    (output/'manifest.toml').write_text(f'hostname = "{host}"\ngg_exe_sha256 = "{sha(patched)}"\nsetup_exe_sha256 = "{sha(installer)}"\n',encoding='utf-8',newline='\n')
    print(json.dumps({'hostname':host,'client_sha256':sha(patched),'installer_sha256':sha(installer),'preserved':'original NSIS stub, scripts, licence and all payload except gg.exe'}))
if __name__=='__main__':main()
