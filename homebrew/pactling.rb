# Generated from release archives by scripts/release-formula.py.
class Pactling < Formula
  desc "A Pact broker in one binary"
  homepage "https://github.com/PatrickPriestley/pactling"
  version "0.1.1"
  license "MIT"

  on_macos do
    depends_on arch: :arm64
    url "https://github.com/PatrickPriestley/pactling/releases/download/v0.1.1/pactling-0.1.1-aarch64-apple-darwin.tar.gz"
    sha256 "5dde14d47d0c21cdf33d1f26676deb8f8796b4a3009cae8171b9f32b7c79f358"
  end

  on_linux do
    depends_on arch: :x86_64
    url "https://github.com/PatrickPriestley/pactling/releases/download/v0.1.1/pactling-0.1.1-x86_64-unknown-linux-gnu.tar.gz"
    sha256 "438b80a3f7db49d8ee3af8b953813b868d224c8892bd37a65f0ec617e8fad3f2"
  end

  def install
    bin.install "pactling"
  end

  test do
    assert_equal "pactling 0.1.1", shell_output("#{bin}/pactling --version").strip
  end
end
