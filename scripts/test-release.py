"""Packaging contract tests; binaries are built and exercised separately."""
import hashlib
import pathlib
import subprocess
import tempfile
import unittest
import tomllib

ROOT = pathlib.Path(__file__).resolve().parents[1]
VERSION = tomllib.loads((ROOT / "Cargo.toml").read_text())["package"]["version"]


class ReleaseTests(unittest.TestCase):
    def test_release_tools_exist(self):
        self.assertTrue((ROOT / 'scripts/package-release.sh').is_file(),
                        'release archive command must exist')
        self.assertTrue((ROOT / 'scripts/release-formula.py').is_file(),
                        'formula must be generated from real archive hashes')

    def test_tag_mismatch_cannot_package(self):
        script = ROOT / 'scripts/package-release.sh'
        self.assertTrue(script.is_file(), 'release archive command must exist')
        p = subprocess.run(['bash', str(script), 'aarch64-apple-darwin', 'v9.9.9'],
                           capture_output=True, text=True)
        self.assertNotEqual(p.returncode, 0)
        self.assertIn('tag does not match Cargo.toml', p.stderr)

    def test_formula_uses_archive_hashes(self):
        script = ROOT / 'scripts/release-formula.py'
        self.assertTrue(script.is_file(), 'formula generator must exist')
        with tempfile.TemporaryDirectory() as directory:
            for target in ['aarch64-apple-darwin', 'x86_64-unknown-linux-gnu']:
                pathlib.Path(directory, f'pactling-{VERSION}-{target}.tar.gz').write_bytes(target.encode())
            p = subprocess.run(['python3', str(script), directory], check=True,
                               capture_output=True, text=True)
            for target in ['aarch64-apple-darwin', 'x86_64-unknown-linux-gnu']:
                self.assertIn(hashlib.sha256(target.encode()).hexdigest(), p.stdout)
                self.assertIn(f'pactling-{VERSION}-{target}.tar.gz', p.stdout)
            self.assertIn(f'assert_equal "pactling {VERSION}"', p.stdout)


if __name__ == '__main__':
    unittest.main()
