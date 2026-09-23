class Cleanrs < Formula
  desc "Safe, rule-based disk cleanup for macOS"
  homepage "https://github.com/ho-doan/my-cleaner"
  license "MIT"

  if Hardware::CPU.arm?
    url "https://github.com/ho-doan/my-cleaner/releases/download/v0.1.5/cleanrs-v0.1.5-aarch64-apple-darwin.tar.gz"
    sha256 "2e152e8ea0166ec73904a25504f37aa1965815f00950b397fc92ef3a7001eb5a"
  else
    url "https://github.com/ho-doan/my-cleaner/releases/download/v0.1.5/cleanrs-v0.1.5-x86_64-apple-darwin.tar.gz"
    sha256 "c7e34a828eb66e7074b4d4d4a6f444d6cc496731d8f9d9ba35a837384b32d642"
  end

  def install
    bin.install "cleanrs"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/cleanrs --version")
  end
end
