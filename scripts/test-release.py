"""Packaging contract tests; binaries are built and exercised separately."""
import hashlib
import pathlib
import subprocess
import tempfile
import unittest
import tomllib
import shutil
import tarfile
import shlex

ROOT = pathlib.Path(__file__).resolve().parents[1]
VERSION = tomllib.loads((ROOT / "Cargo.toml").read_text())["package"]["version"]


class ReleaseTests(unittest.TestCase):
    def test_ci_checks_default_and_published_feature_sets(self):
        commands = [line.split('run:', 1)[1].strip()
                    for line in (ROOT / '.github/workflows/ci.yml').read_text().splitlines()
                    if '- run:' in line]
        for command in ['cargo clippy --all-targets -- -D warnings', 'cargo test',
                        'cargo clippy --all-targets --features mcp -- -D warnings',
                        'cargo test --features mcp', 'python3 scripts/test-release.py']:
            with self.subTest(command=command):
                self.assertIn(command, commands, 'CI must check source and published configurations')

    def test_published_native_builds_enable_mcp(self):
        workflow = (ROOT / '.github/workflows/release.yml').read_text()
        builds = [shlex.split(line.split('run:', 1)[1])
                  for line in workflow.splitlines() if 'run: cargo build ' in line]
        self.assertEqual(len(builds), 2)
        for command in builds:
            self.assertIn('--features', command, 'published binaries must enable MCP')
            self.assertIn('mcp', command[command.index('--features') + 1].split(','))
        features = tomllib.loads((ROOT / 'Cargo.toml').read_text())['features']
        self.assertEqual(features['default'], [], 'source default remains feature-disabled')

    def test_published_image_enables_mcp_and_copies_schemas(self):
        dockerfile = (ROOT / 'Dockerfile').read_text()
        build = next(line for line in dockerfile.splitlines() if line.startswith('RUN cargo build'))
        command = shlex.split(build.split('&&', 1)[0])[1:]
        self.assertIn('--features', command, 'published image must enable MCP')
        self.assertIn('mcp', command[command.index('--features') + 1].split(','))
        self.assertIn('COPY docs/api/mcp ./docs/api/mcp', dockerfile.splitlines())

    def test_image_context_includes_only_mcp_schema_documentation(self):
        rules = (ROOT / '.dockerignore').read_text().splitlines()
        for rule in ['!docs/', '!docs/api/', '!docs/api/mcp/', '!docs/api/mcp/*.schema.json']:
            self.assertIn(rule, rules, 'embedded MCP schemas must reach Docker build context')
        documentation_rules = {rule for rule in rules if rule.startswith('!docs')}
        self.assertEqual(documentation_rules,
                         {'!docs/', '!docs/api/', '!docs/api/mcp/', '!docs/api/mcp/*.schema.json'})

    def test_archives_include_license(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            (root / 'scripts').mkdir()
            shutil.copy(ROOT / 'scripts/package-release.sh', root / 'scripts')
            shutil.copy(ROOT / 'Cargo.toml', root)
            (root / 'LICENSE').write_text('license notice')
            binary = root / 'target/aarch64-apple-darwin/release/pactling'
            binary.parent.mkdir(parents=True)
            binary.write_text('test binary')
            subprocess.run(['bash', str(root / 'scripts/package-release.sh'),
                            'aarch64-apple-darwin', f'v{VERSION}'], check=True)
            with tarfile.open(next((root / 'dist').glob('*.tar.gz'))) as archive:
                self.assertIn('LICENSE', archive.getnames())
                self.assertEqual(archive.extractfile('LICENSE').read(), b'license notice')

    def test_image_tags_keep_prereleases_off_latest(self):
        script = ROOT / 'scripts/release-tags.py'
        self.assertTrue(script.is_file(), 'release image tag generator must exist')
        for tag, latest in [('v0.1.0', True), ('v0.2.0-rc1', False)]:
            result = subprocess.run(['python3', str(script), tag], check=True,
                                    capture_output=True, text=True)
            tags = result.stdout.splitlines()
            self.assertEqual(tags[0], "value<<PACTLING_TAGS")
            self.assertEqual(tags[-2], "PACTLING_TAGS")
            self.assertIn(f'ghcr.io/patrickpriestley/pactling:{tag}', tags)
            self.assertEqual('ghcr.io/patrickpriestley/pactling:latest' in tags, latest)
            self.assertIn(f'prerelease={str(not latest).lower()}', tags)

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
