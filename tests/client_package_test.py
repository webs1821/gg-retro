"""Exercise the real installer builder without executing any Windows binary."""
import hashlib
import importlib.util
import pathlib
import subprocess
import sys
import tempfile
import tomllib
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('builder', ROOT / 'tools/build_client.py')
builder = importlib.util.module_from_spec(spec)
spec.loader.exec_module(builder)


class ClientPackageTests(unittest.TestCase):
    def test_real_client_fixes_and_input_guards(self):
        with tempfile.TemporaryDirectory(prefix='gg-client-fixes-') as tmp:
            directory = pathlib.Path(tmp)
            stream = builder.unpack((ROOT / 'server/static/gg61.exe').read_bytes())[3]
            original = directory / 'gg.exe'
            original.write_bytes(builder.extract_client(stream))
            header = directory / 'installer-header.bin'
            header.write_bytes(stream[:4+int.from_bytes(stream[:4], 'little')])
            result = subprocess.run(['node', str(ROOT / 'tests/client_fixes.test.cjs'),
                                     str(original), str(header)], capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            print(result.stdout.strip())

    def test_rebuild_and_reject_wrong_inputs(self):
        with tempfile.TemporaryDirectory(prefix='gg-package-') as tmp:
            root = pathlib.Path(tmp)
            config = root / 'config.toml'
            config.write_text('hostname = "gg.example.org"\n')
            output = root / 'package'
            command = [sys.executable, str(ROOT / 'tools/build_client.py'),
                       '--config', str(config), '--output', str(output)]
            subprocess.run(command, check=True, capture_output=True)
            manifest = tomllib.loads((output / 'manifest.toml').read_text())
            original = (ROOT / 'server/static/gg61.exe').read_bytes()
            client = (output / 'gg.exe').read_bytes()
            installer = (output / 'gg-setup.exe').read_bytes()
            self.assertEqual(manifest['hostname'], 'gg.example.org')
            self.assertEqual(manifest['gg_exe_sha256'], builder.sha(client))
            self.assertEqual(manifest['setup_exe_sha256'], builder.sha(installer))
            offset, _, _, before = builder.unpack(original)
            new_offset, _, _, after = builder.unpack(installer)
            self.assertEqual(offset, new_offset)
            self.assertEqual(installer[:offset], original[:offset])
            self.assertEqual(after.count(client), 1)
            self.assertEqual(after.replace(client, builder.extract_client(before)), before)
            self.assertIn(b'gg.example.org', client)
            self.assertNotEqual(builder.sha(client), builder.ORIGINAL_CLIENT_SHA256)
            self.assertEqual(client[0x841bc:0x841cf], b'wiadomosc\0dostepny\0')
            self.assertEqual(client[0x45f80:0x45f92], bytes.fromhex('31c0c3909090') * 3)
            # Never overwrite a previous release, even with the same configuration.
            self.assertNotEqual(subprocess.run(command, capture_output=True).returncode, 0)
            self.assertEqual((output / 'gg-setup.exe').read_bytes(), installer)
            # A client for a different domain must not be packaged accidentally.
            config.write_text('hostname = "gg.example.net"\n')
            wrong = command[:-1] + [str(root / 'wrong'), '--patched-exe', str(output / 'gg.exe')]
            self.assertNotEqual(subprocess.run(wrong, capture_output=True).returncode, 0)
            self.assertFalse((root / 'wrong/gg.exe').exists())
            config.write_text('hostname = "this-domain-is-too-long.example"\n')
            self.assertNotEqual(subprocess.run(command[:-1] + [str(root / 'long')], capture_output=True).returncode, 0)
            corrupt = bytearray(original)
            corrupt[-1] ^= 1
            with self.assertRaises(AssertionError):
                builder.unpack(corrupt)
            with self.assertRaises(AssertionError):
                builder.rebuild(corrupt, client)


if __name__ == '__main__':
    unittest.main()
