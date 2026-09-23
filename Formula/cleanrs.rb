class Cleanrs < Formula
  desc "Safe, rule-based disk cleanup for macOS"
  homepage "https://github.com/ho-doan/my-cleaner"
  url "https://github.com/ho-doan/my-cleaner/archive/refs/tags/v0.1.0.tar.gz"
  license "MIT"

  depends_on "rust" => :build

  def install
    system "cargo", "install", "--locked", "--path", "crates/cleanrs-cli", "--root", prefix
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/cleanrs --version")
  end
end
