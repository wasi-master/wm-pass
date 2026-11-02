cask "wm-pass" do
  version "0.1.0"
  sha256 "0000000000000000000000000000000000000000000000000000000000000000"

  url "https://github.com/wasi-master/wm-pass/releases/download/desktop-v#{version}/WM.Pass_#{version}_universal.dmg"
  name "WM Pass"
  desc "Password manager that keeps its vault on your own devices"
  homepage "https://github.com/wasi-master/wm-pass"

  livecheck do
    url :url
    strategy :github_latest
    regex(/^desktop[._-]v?(\d+(?:\.\d+)+)$/i)
  end

  depends_on macos: ">= :big_sur"

  app "WM Pass.app"

  # The app is not notarised, because the suite has no Apple Developer account.
  postflight do
    system_command "/usr/bin/xattr", args: ["-dr", "com.apple.quarantine", "#{appdir}/WM Pass.app"]
  end

  zap trash: [
    "~/.wm-pass",
    "~/Library/Application Support/com.wasimaster.wmpass.desktop",
    "~/Library/Caches/com.wasimaster.wmpass.desktop",
    "~/Library/WebKit/com.wasimaster.wmpass.desktop",
  ]
end
