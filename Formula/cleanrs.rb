class Cleanrs < Formula
  desc "Safe, rule-based disk cleanup for macOS"
  homepage "https://github.com/ho-doan/my-cleaner"
  license "MIT"

  if Hardware::CPU.arm?
    url "https://github.com/ho-doan/my-cleaner/releases/download/v0.1.7/cleanrs-v0.1.7-aarch64-apple-darwin.tar.gz"
    sha256 "8d227f0ab28d9e5ef9216a36bf48d36de8a93ca6b99e0337626932d81eb72969"
  else
    url "https://github.com/ho-doan/my-cleaner/releases/download/v0.1.7/cleanrs-v0.1.7-x86_64-apple-darwin.tar.gz"
    sha256 "900774a463e503fc604abb791797ce547702bcf6ef83f16e7858aadb7130f5f0"
  end

  def install
    bin.install "cleanrs"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/cleanrs --version")
  end
end
