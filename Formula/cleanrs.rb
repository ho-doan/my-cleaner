class Cleanrs < Formula
  desc "Safe, rule-based disk cleanup for macOS"
  homepage "https://github.com/ho-doan/my-cleaner"
  license "MIT"

  if Hardware::CPU.arm?
    url "https://github.com/ho-doan/my-cleaner/releases/download/v0.1.2/cleanrs-v0.1.2-aarch64-apple-darwin.tar.gz"
  else
    url "https://github.com/ho-doan/my-cleaner/releases/download/v0.1.2/cleanrs-v0.1.2-x86_64-apple-darwin.tar.gz"
  end

  def install
    bin.install "cleanrs"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/cleanrs --version")
  end
end
