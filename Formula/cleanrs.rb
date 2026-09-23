class Cleanrs < Formula
  desc "Safe, rule-based disk cleanup for macOS"
  homepage "https://github.com/ho-doan/my-cleaner"
  license "MIT"

  if Hardware::CPU.arm?
    url "https://github.com/ho-doan/my-cleaner/releases/download/v0.1.3/cleanrs-v0.1.3-aarch64-apple-darwin.tar.gz"
    sha256 "fb71f116f771bf17c89d5e53a7f65cc515b4fa98b4836eccae9dc88982207bd7"
  else
    url "https://github.com/ho-doan/my-cleaner/releases/download/v0.1.3/cleanrs-v0.1.3-x86_64-apple-darwin.tar.gz"
    sha256 "559fab41ec179eb26aeb31875f7c4cc6f8ecba686e8e1454d0d81b3c307a9206"
  end

  def install
    bin.install "cleanrs"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/cleanrs --version")
  end
end
