# Generated from release archives by scripts/release-formula.py.
class Pactling < Formula
  desc "A Pact broker in one binary"
  homepage "https://github.com/PatrickPriestley/pactling"
  version "0.1.0"
  license "MIT"

  on_macos do
    depends_on arch: :arm64
    url "https://github.com/PatrickPriestley/pactling/releases/download/v0.1.0/pactling-0.1.0-aarch64-apple-darwin.tar.gz"
    sha256 "bda2554f5770b9f47ec65c6f194cca421f7f93e9ad617d2618c7e18570443e6e"
  end

  on_linux do
    depends_on arch: :x86_64
    url "https://github.com/PatrickPriestley/pactling/releases/download/v0.1.0/pactling-0.1.0-x86_64-unknown-linux-gnu.tar.gz"
    sha256 "e98501a89efd58e18c48424b8a4b59f397ba7bb9c9e2bf59f9dc9919d72f8312"
  end

  def install
    bin.install "pactling"
  end

  test do
    assert_equal "pactling 0.1.0", shell_output("#{bin}/pactling --version").strip
  end
end
