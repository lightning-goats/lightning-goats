"""Actual protected website-file transactions; nginx/backend readiness mocked."""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

import test_upgrade_contact_prepare as preparation

SCRIPTS = Path(__file__).resolve().parents[1] / 'scripts'
with patch.dict(sys.modules, {'inactive_file_transaction': preparation.TX}):
    spec = importlib.util.spec_from_file_location('self_read_site_fixture', SCRIPTS / 'install-cyberherd-self-read-site.py')
    E = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(E)

FLAGS = 'window.LightningGoatsSite = Object.freeze({ paymentsEnabled: true, feedingAvailable: false, cyberherdEnabled: true, cyberherdAuthEnabled: false, contactPaymentsEnabled: true });\n'


class SelfReadSiteBoundaries(unittest.TestCase):
    def test_only_auth_flag_changes_including_preserved_contact_and_feeding_choices(self):
        changed = E.site_flags(FLAGS)
        self.assertEqual(changed, FLAGS.replace('cyberherdAuthEnabled: false', 'cyberherdAuthEnabled: true'))
        for text in (changed, FLAGS+'extra()', FLAGS.replace('feedingAvailable: false,', ''),
                     FLAGS.replace('cyberherdEnabled: true', 'cyberherdEnabled: false'),
                     FLAGS.replace('feedingAvailable: false', 'feedingAvailable: f()'),
                     FLAGS.replace('feedingAvailable: false', 'feedingAvailable: false, feedingAvailable: true')):
            with self.subTest(text=text), self.assertRaises(ValueError):
                E.site_flags(text)

    def test_site_change_only_switches_fixed_root_and_adds_the_self_read_include(self):
        old = '    root '+str(E.WWW/('contact-'+'a'*40))+';\n'+E.ANCHOR+'    location / { try_files $uri =404; }\n'
        original, release, new = E.site_change(old, 'b'*40)
        self.assertEqual(release, E.WWW/('self-read-'+'b'*40))
        self.assertEqual(new.replace(str(release),str(original)).replace('    include '+str(E.SNIPPET)+';\n',''),old)
        for value in (old+old, old.replace(E.ANCHOR,''),old.replace(str(E.WWW),'/etc'),new):
            with self.assertRaises(ValueError):E.site_change(value,'b'*40)

    def test_direct_execution_and_missing_activation_refuse_before_host_access(self):
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary)
            shutil.copyfile(SCRIPTS/'install-cyberherd-self-read-site.py',root/'site.py')
            (root/'install-private-contact-site.py').write_text('raise RuntimeError("INJECTED")')
            r=subprocess.run(['/usr/bin/python3','-I','-S','-B',str(root/'site.py'),'--help'],capture_output=True,text=True,timeout=10)
            self.assertNotEqual(r.returncode,0);self.assertIn('externally verified root-owned tool launcher',r.stderr)
            self.assertNotIn('INJECTED',r.stderr)
        with patch.object(E.C,'root',side_effect=AssertionError('host accessed')):
            with self.assertRaisesRegex(ValueError,'explicit self-read site activation'):
                E.apply(Path('/unused'),'a'*64,False)


@unittest.skipUnless(os.geteuid()==0,'actual protected website files require separate root CI')
class SelfReadSiteFiles(unittest.TestCase):
    def setUp(self):
        previous=os.umask(0o077);self.addCleanup(os.umask,previous)
        temporary=tempfile.TemporaryDirectory(prefix='lg-self-read-site-',dir='/run');self.addCleanup(temporary.cleanup)
        self.root=Path(temporary.name)
        self.www,self.base,self.etc=(self.root/n for n in ('www','upgrades','etc'))
        for path in (self.www,self.base,self.etc):path.mkdir(mode=0o700)
        self.original=self.www/('contact-'+'a'*40);self.original.mkdir(mode=0o755);self.original.chmod(0o755)
        self.site,self.snippet=self.etc/'site.conf',self.etc/'self-read.conf'
        self.write(self.site,('    root '+str(self.original)+';\n'+E.ANCHOR).encode())
        self.original_site=self.site.read_bytes()
        names=E.BROWSER|{'index.html','site-config.js','cyberherd-auth.js','overlay.html','overlay.js','overlay.css','images/goat.png','contact.js','contact-fee.js'}
        for name in names:
            path=self.original/name;path.parent.mkdir(mode=0o755,exist_ok=True);path.parent.chmod(0o755)
            self.write(path,(FLAGS if name=='site-config.js' else 'original '+name).encode())
        self.identity={'source_commit':'b'*40,'manifest_sha256':'c'*64}
        self.binding={'private_backend_identity':'original'}
        self.calls=[];self.checks=0;self.reloads=0;self.fail_new_config=False;self.fail_first_reload=False
        for obj,name,value in [(E,'WWW',self.www),(E,'SITE',self.site),(E,'SNIPPET',self.snippet),
            (E.C,'BASE',self.base),(E.C,'root',lambda:None),(E.M,'C',E.C),
            (E,'verified_tool_identity',lambda:self.identity),(E,'ready',lambda:dict(self.binding)),(E.C,'command',self.command)]:
            context=patch.object(obj,name,value);context.start();self.addCleanup(context.stop)
        self.before=E.P.inventory(self.original)
        self.payload=self.base/'payload';self.payload.mkdir(mode=0o700)
        for name in E.PAYLOAD:self.write(self.payload/name,('qualified '+name).encode(),0o600)
        manifest={'version':1,'source_commit':'b'*40,'files':{n:hashlib.sha256((self.payload/n).read_bytes()).hexdigest() for n in E.PAYLOAD},
                  'browser_files':{n:self.before[n]['sha256'] for n in E.BROWSER}}
        self.write(self.payload/'SOURCE.json',json.dumps(manifest).encode(),0o600)
        self.directory=self.base/'prepared';self.directory.mkdir(mode=0o700)

    @staticmethod
    def write(path,data,mode=0o644):
        path.write_bytes(data);path.chmod(mode)

    def command(self,*args,**kwargs):
        self.calls.append(args)
        if args==('/usr/sbin/nginx','-t'):
            self.checks+=1
            if self.fail_new_config and self.checks==2:raise ValueError('synthetic new nginx validation refusal')
        elif args==('/usr/bin/systemctl','reload','nginx.service'):
            self.reloads+=1
            if self.fail_first_reload and self.reloads==1:raise ValueError('synthetic nginx reload refusal')
        else:raise AssertionError('unexpected application/service command')
        return ''

    def prepare(self):
        result=E.prepare(self.payload,E.U.digest(self.payload/'SOURCE.json'),self.directory)
        self.assertEqual(result,{'prepared':True,'services_changed':False,'self_read_site_enabled':False,'operator_ingress_enabled':False})
        self.assertEqual(self.calls,[]);self.assertEqual(self.site.read_bytes(),self.original_site)
        return E.U.digest(self.directory/'PREPARED.json')

    def test_minimal_release_preserves_all_other_bytes_metadata_and_flags(self):
        digest=self.prepare();release=self.www/('self-read-'+'b'*40)
        self.assertEqual(release.stat().st_mode&0o777,0o755)
        self.assertEqual((release/'vendor').stat().st_mode&0o777,0o755)
        for name,fp in self.before.items():
            if name not in ('site-config.js','cyberherd-auth.js'):self.assertEqual(E.TX.fingerprint(release/name),fp)
            else:
                new=E.TX.fingerprint(release/name)
                self.assertEqual({k:v for k,v in new.items() if k!='sha256'},{k:v for k,v in fp.items() if k!='sha256'})
        self.assertEqual((release/'site-config.js').read_text(),E.site_flags(FLAGS))
        self.assertTrue(E.apply(self.directory,digest,True)['self_read_site_enabled'])
        self.assertEqual(self.calls,[('/usr/sbin/nginx','-t'),('/usr/sbin/nginx','-t'),('/usr/bin/systemctl','reload','nginx.service')])
        self.assertEqual(E.P.inventory(self.original),self.before)
        with self.assertRaisesRegex(ValueError,'already began'):E.apply(self.directory,digest,True)

    def test_failed_validation_restores_only_site_without_reload(self):
        digest=self.prepare();self.fail_new_config=True
        with self.assertRaisesRegex(ValueError,'validation refusal'):E.apply(self.directory,digest,True)
        self.assertEqual(self.site.read_bytes(),self.original_site);self.assertEqual(self.reloads,0)
        self.assertTrue((self.directory/'CONFIGURATION-RESTORED.json').exists())
        self.assertEqual(E.P.inventory(self.original),self.before)

    def test_failed_reload_restores_original_site_and_reloads_only_nginx(self):
        digest=self.prepare();self.fail_first_reload=True
        with self.assertRaisesRegex(ValueError,'reload refusal'):E.apply(self.directory,digest,True)
        self.assertEqual(self.site.read_bytes(),self.original_site);self.assertEqual(self.reloads,2)
        self.assertTrue((self.directory/'CONFIGURATION-RESTORED.json').exists())
        self.assertEqual(E.P.inventory(self.original),self.before)

    def test_backend_or_release_drift_prevents_any_configuration_replacement(self):
        digest=self.prepare();self.binding['private_backend_identity']='changed'
        with self.assertRaisesRegex(ValueError,'application identity'):E.apply(self.directory,digest,True)
        self.binding['private_backend_identity']='original'
        (self.www/('self-read-'+'b'*40)/'images/goat.png').write_bytes(b'changed artwork')
        with self.assertRaisesRegex(ValueError,'reviewed self-read website'):E.apply(self.directory,digest,True)
        self.assertEqual(self.calls,[]);self.assertEqual(self.site.read_bytes(),self.original_site)

    def test_source_browser_mismatch_refuses_before_release_creation(self):
        (self.original/'vendor/nostr.bundle.js').write_bytes(b'unknown browser crypto')
        with self.assertRaisesRegex(ValueError,'qualified source'):E.prepare(self.payload,E.U.digest(self.payload/'SOURCE.json'),self.directory)
        self.assertFalse((self.www/('self-read-'+'b'*40)).exists())

    def test_payload_alias_or_wrong_purpose_cannot_activate(self):
        auth=self.payload/'cyberherd-auth.js';auth.unlink();auth.symlink_to(self.original/'cyberherd-auth.js')
        with self.assertRaises(ValueError):E.prepare(self.payload,E.U.digest(self.payload/'SOURCE.json'),self.directory)
        auth.unlink();self.write(auth,b'qualified cyberherd-auth.js',0o600)
        digest=self.prepare();record=self.directory/'PREPARED.json';value=json.loads(record.read_text())
        value['operator_ingress_enabled']=0;record.write_text(json.dumps(value))
        with self.assertRaisesRegex(ValueError,'self-read-only'):E.apply(self.directory,E.U.digest(record),True)
        self.assertEqual(self.calls,[])
