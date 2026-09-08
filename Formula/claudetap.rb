class Claudetap < Formula
  desc "Capture AI traffic locally and sync sessions to an organization server"
  homepage "https://github.com/leanmcp-community/claudetap"
  license "MIT"
  head "https://github.com/leanmcp-community/claudetap.git", branch: "main"

  depends_on "rust" => :build
  depends_on :macos

  def install
    system "cargo", "install", *std_cargo_args(path: buildpath), "--locked"
  end

  service do
    run [opt_bin/"claudetap", "cloud", "sync"]
    keep_alive successful_exit: false
    log_path var/"log/claudetap-sync.log"
    error_log_path var/"log/claudetap-sync.log"
  end

  def caveats
    <<~EOS
      Configure your server and upload key before starting the uploader:
        export CLAUDETAP_UPLOAD_KEY='your-key'
        claudetap cloud configure --endpoint https://your-server
        unset CLAUDETAP_UPLOAD_KEY
        brew services start leanmcp-community/claudetap/claudetap

      Run without sudo to use your own ~/.claudetap captures and credentials.
      Stop any older Cargo-installed uploader service before starting this one.
      Check `which claudetap` if an older Cargo binary shadows this installation.
    EOS
  end

  test do
    assert_match "cloud", shell_output("#{bin}/claudetap --help")
    assert_match "sync", shell_output("#{bin}/claudetap cloud --help")
    assert_match "already stopped", shell_output("CLAUDETAP_HOME=#{testpath}/capture #{bin}/claudetap cloud stop")
  end
end
