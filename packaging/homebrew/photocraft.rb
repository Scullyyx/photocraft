# Homebrew cask for PhotoCraft, published in the storytold/homebrew-tap tap:
#   brew install --cask storytold/tap/photocraft
# Source: https://github.com/storytold/photocraft/tree/main/packaging/homebrew
# (packaging/homebrew/update.sh sets version and sha256 for each release.)
cask "photocraft" do
  version "0.3.0"
  sha256 "c0b0223cddb18dd7f5607fb4d6cc6a925a62f5b5b47457997d61ad0f72aa5911"

  url "https://github.com/storytold/photocraft/releases/download/v#{version}/photocraft-#{version}-macos-universal.dmg",
      verified: "github.com/storytold/photocraft/"
  name "PhotoCraft"
  desc "Image editor with layers, masks, type and PSD files"
  homepage "https://getartcraft.com/apps/photocraft"

  livecheck do
    url :url
    strategy :github_latest
  end

  # LSMinimumSystemVersion in packaging/macos/Info.plist.in.
  depends_on macos: ">= :big_sur"

  app "PhotoCraft.app"

  uninstall quit: "ai.storyteller.photocraft"

  # Preferences, presets, recovery autosaves and window layout (apps/photocraft/src/app_dirs.rs),
  # plus what macOS keeps per bundle id.
  zap trash: [
    "~/Library/Application Support/Photocraft",
    "~/Library/Preferences/ai.storyteller.photocraft.plist",
    "~/Library/Saved Application State/ai.storyteller.photocraft.savedState",
  ]
end
