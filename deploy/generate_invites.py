#!/usr/bin/env python3
"""Generate a private invitation file; never overwrite an existing pool or print codes."""
import argparse, os, secrets
from pathlib import Path

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--count',type=int,default=100)
    parser.add_argument('--output',type=Path,required=True)
    args=parser.parse_args()
    if not 1 <= args.count <= 10000:parser.error('count must be between 1 and 10000')
    os.umask(0o077)
    args.output.parent.mkdir(mode=0o700,parents=True,exist_ok=True)
    codes=set()
    while len(codes)<args.count:codes.add('GG-'+secrets.token_urlsafe(24))
    # O_EXCL prevents accidental replacement of an existing invitation pool.
    with args.output.open('x',encoding='ascii',newline='\n') as output:
        output.write('\n'.join(sorted(codes))+'\n')
    print(f'Generated {len(codes)} private codes, each allowing 3 successful registrations.')
if __name__=='__main__':main()
