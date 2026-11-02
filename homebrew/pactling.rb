# Generated from release archives by scripts/release-formula.py.
class Pactling < Formula
  desc "A Pact broker in one binary"
  homepage "https://github.com/PatrickPriestley/pactling"
  version "0.1.2"
  license "MIT"

  on_macos do
    depends_on arch: :arm64
    url "https://github.com/PatrickPriestley/pactling/releases/download/v0.1.2/pactling-0.1.2-aarch64-apple-darwin.tar.gz"
    sha256 "312cca55a21eea887cb27e94e5437bb52760231dbd9b5485c0fbcd719ed3df9b"
  end

  on_linux do
    depends_on arch: :x86_64
    url "https://github.com/PatrickPriestley/pactling/releases/download/v0.1.2/pactling-0.1.2-x86_64-unknown-linux-gnu.tar.gz"
    sha256 "5716b4aa48e62b9b0ae8426ea17d421ccce8e2cc380ce3f20ae697dea78a438f"
  end

  def install
    bin.install "pactling"
  end

  test do
    assert_equal "pactling 0.1.2", shell_output("#{bin}/pactling --version").strip
  end
end
