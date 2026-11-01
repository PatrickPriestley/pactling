# Generated from release archives by scripts/release-formula.py.
class Pactling < Formula
  desc "A Pact broker in one binary"
  homepage "https://github.com/PatrickPriestley/pactling"
  version "0.1.0"
  license "MIT"

  on_macos do
    depends_on arch: :arm64
    url "https://github.com/PatrickPriestley/pactling/releases/download/v0.1.0/pactling-0.1.0-aarch64-apple-darwin.tar.gz"
    sha256 "9e3e6e307ac402ecd509b35e2ce6af59829965b4c28458d29bcb61136d4fa560"
  end

  on_linux do
    depends_on arch: :x86_64
    url "https://github.com/PatrickPriestley/pactling/releases/download/v0.1.0/pactling-0.1.0-x86_64-unknown-linux-gnu.tar.gz"
    sha256 "f9b091485c8eab81d1f89aad711da5b41b14f7e7af64a9672e3ff008940a2de5"
  end

  def install
    bin.install "pactling"
  end

  test do
    assert_equal "pactling 0.1.0", shell_output("#{bin}/pactling --version").strip
  end
end
