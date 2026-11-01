#!/usr/bin/env python3
"""Render a tap-ready formula from the exact release archives (Python 3.11+)."""
import hashlib
from pathlib import Path
import sys
import tomllib

root = Path(__file__).resolve().parents[1]
version = tomllib.loads((root / "Cargo.toml").read_text())["package"]["version"]
dist = Path(sys.argv[1])
url = f"https://github.com/PatrickPriestley/pactling/releases/download/v{version}"
mac = f"pactling-{version}-aarch64-apple-darwin.tar.gz"
linux = f"pactling-{version}-x86_64-unknown-linux-gnu.tar.gz"
mac_sha = hashlib.sha256((dist / mac).read_bytes()).hexdigest()
linux_sha = hashlib.sha256((dist / linux).read_bytes()).hexdigest()
print(f'''# Generated from release archives by scripts/release-formula.py.
class Pactling < Formula
  desc "A Pact broker in one binary"
  homepage "https://github.com/PatrickPriestley/pactling"
  version "{version}"
  license "MIT"

  on_macos do
    depends_on arch: :arm64
    url "{url}/{mac}"
    sha256 "{mac_sha}"
  end

  on_linux do
    depends_on arch: :x86_64
    url "{url}/{linux}"
    sha256 "{linux_sha}"
  end

  def install
    bin.install "pactling"
  end

  test do
    assert_equal "pactling {version}", shell_output("#{{bin}}/pactling --version").strip
  end
end''')
