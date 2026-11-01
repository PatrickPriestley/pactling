# Generated from release archives by scripts/release-formula.py.
class Pactling < Formula
  desc "A Pact broker in one binary"
  homepage "https://github.com/PatrickPriestley/pactling"
  version "0.1.0"
  license "MIT"

  on_macos do
    depends_on arch: :arm64
    url "https://github.com/PatrickPriestley/pactling/releases/download/v0.1.0/pactling-0.1.0-aarch64-apple-darwin.tar.gz"
    sha256 "c3f91ec7072fa1dfe868f32c00c9b0d96b8aa163dfdee891179fce7ef3ff2f10"
  end

  on_linux do
    depends_on arch: :x86_64
    url "https://github.com/PatrickPriestley/pactling/releases/download/v0.1.0/pactling-0.1.0-x86_64-unknown-linux-gnu.tar.gz"
    sha256 "efb38ac2f5811e740010290ce67b15ee6f96643aabd8692c512810dbb523a23e"
  end

  def install
    bin.install "pactling"
  end

  test do
    assert_equal "pactling 0.1.0", shell_output("#{bin}/pactling --version").strip
  end
end
