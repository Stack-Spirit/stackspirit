# typed: false
# frozen_string_literal: true

# Private formula for StackSpirit (formerly VOWP): stackspirit, vowp and both daemons.
# Nothing publishes this file; the private release URL needs the owner's access.
class Stackspirit < Formula
  desc "StackSpirit: local-first supervisor for AI coding agents"
  homepage "https://github.com/Stack-Spirit/stackspirit-engine"
  version "__STACKSPIRIT_VERSION__"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/Stack-Spirit/stackspirit-engine/releases/download/v#{version}/stackspirit-#{version}-aarch64-apple-darwin.tar.gz"
      sha256 "__SHA256_AARCH64_APPLE_DARWIN__"
    end
    on_intel do
      url "https://github.com/Stack-Spirit/stackspirit-engine/releases/download/v#{version}/stackspirit-#{version}-x86_64-apple-darwin.tar.gz"
      sha256 "__SHA256_X86_64_APPLE_DARWIN__"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/Stack-Spirit/stackspirit-engine/releases/download/v#{version}/stackspirit-#{version}-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "__SHA256_AARCH64_UNKNOWN_LINUX_GNU__"
    end
    on_intel do
      url "https://github.com/Stack-Spirit/stackspirit-engine/releases/download/v#{version}/stackspirit-#{version}-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "__SHA256_X86_64_UNKNOWN_LINUX_GNU__"
    end
  end

  def install
    first = Dir["stackspirit-*/stackspirit"].first
    root = first ? File.dirname(first) : "."
    # Both CLIs and both daemons; the archive carries all four.
    %w[stackspirit vowp stackspirit-daemon vowp-daemon].each do |name|
      bin.install File.join(root, name)
    end
    %w[README.md LICENSE].each do |name|
      path = File.join(root, name)
      doc.install path if File.exist?(path)
    end
  end

  test do
    assert_match "stackspirit #{version}", shell_output("#{bin}/stackspirit --version")
    assert_match "vowp #{version}", shell_output("#{bin}/vowp --version")
  end
end
