class Cleanrs < Formula
  desc "Safe, rule-based disk cleanup for macOS"
  homepage "https://github.com/ho-doan/my-cleaner"
  license "MIT"

  if Hardware::CPU.arm?
    url "https://github.com/ho-doan/my-cleaner/releases/download/v0.1.4/cleanrs-v0.1.4-aarch64-apple-darwin.tar.gz"
    sha256 "8990b85b1e61ad61f06c1c334a8942a09abd71e0f65f482f45c4c670ed7b7449"
  else
    url "https://github.com/ho-doan/my-cleaner/releases/download/v0.1.4/cleanrs-v0.1.4-x86_64-apple-darwin.tar.gz"
    sha256 "bb4ba3ba319a99f1c7132dc834995f6ddbfd78cbe0f923c7e4e3bd8f0e2d174d"
  end

  def install
    bin.install "cleanrs"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/cleanrs --version")
  end
end
