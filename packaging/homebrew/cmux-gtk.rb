cask "cmux-gtk" do
  version "0.1.0"
  sha256 "REPLACE_WITH_RELEASE_SHA256"

  url "https://github.com/nitecon/cmux-gtk/releases/download/v#{version}/cmux-gtk-linux-x86_64.tar.gz"
  name "cmux GTK"
  desc "GPU-accelerated terminal multiplexer powered by Ghostty"
  homepage "https://github.com/nitecon/cmux-gtk"

  livecheck do
    url :url
    strategy :github_latest
  end

  depends_on :linux
  depends_on arch: :x86_64
  depends_on formula: %w[
    fontconfig
    freetype
    gtk4
    libxml2
    llvm
    oniguruma
  ]

  command_wrapper "cmux",
                  content: <<~SH
                    #!/bin/sh
                    export LD_LIBRARY_PATH="#{HOMEBREW_PREFIX}/opt/llvm/lib:#{HOMEBREW_PREFIX}/opt/libxml2/lib:#{HOMEBREW_PREFIX}/opt/gtk4/lib:#{HOMEBREW_PREFIX}/opt/fontconfig/lib:#{HOMEBREW_PREFIX}/opt/freetype/lib:#{HOMEBREW_PREFIX}/opt/oniguruma/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
                    exec "#{staged_path}/cmux" "$@"
                  SH
  command_wrapper "cmux-app",
                  content: <<~SH
                    #!/bin/sh
                    export LD_LIBRARY_PATH="#{HOMEBREW_PREFIX}/opt/llvm/lib:#{HOMEBREW_PREFIX}/opt/libxml2/lib:#{HOMEBREW_PREFIX}/opt/gtk4/lib:#{HOMEBREW_PREFIX}/opt/fontconfig/lib:#{HOMEBREW_PREFIX}/opt/freetype/lib:#{HOMEBREW_PREFIX}/opt/oniguruma/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
                    exec "#{staged_path}/cmux-app" "$@"
                  SH
end
