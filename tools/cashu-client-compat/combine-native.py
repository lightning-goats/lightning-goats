#!/usr/bin/env python3
"""Combine qualified synthetic native CLI outputs for independent parser tests."""
import argparse
import json
from pathlib import Path


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('short',type=Path)
    parser.add_argument('long',type=Path)
    parser.add_argument('output',type=Path)
    args=parser.parse_args()
    first,second=[json.loads(path.read_text()) for path in [args.short,args.long]]
    if first['binary_sha256']!=second['binary_sha256']:
        raise ValueError('native fixture binaries differ')
    combined={}
    for prefix,value in [('',first),('long_',second)]:
        if (value['result']!='CASHU_REQUEST_CLI_VERIFIED' or not value['synthetic_only']
                or value['state_created'] or value['services_started']):
            raise ValueError('native source is not an inactive synthetic request fixture')
        if bool(value.get('long_request',False))!=bool(prefix):
            raise ValueError('short/long native fixtures reversed')
        if set(value['requests'])!={'creqA','creqB'}:
            raise ValueError('both encodings required')
        if value['requests']['creqA']['id']!=value['requests']['creqB']['id']:
            raise ValueError('encoding changed descriptor identity')
        combined.update({prefix+key:request for key,request in value['requests'].items()})
    args.output.write_text(json.dumps({'warning':'Synthetic native CLI requests; never fund.',
        'source_binary_sha256':first['binary_sha256'],'requests':combined},indent=2)+'\n')


if __name__=='__main__':
    main()
