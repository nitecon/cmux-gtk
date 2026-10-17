# Prepare the MSVC GTK/Ghostty build in repository-local .tools directories.
# Dot-source this script to retain its environment for subsequent cargo commands.
[CmdletBinding()]
param([switch]$SkipBackend)

$ErrorActionPreference = 'Stop'
if ($PSVersionTable.PSVersion.Major -lt 7) { throw 'Run this script with PowerShell 7 (pwsh).' }
$PSNativeCommandUseErrorActionPreference = $true
$repository = Split-Path $PSScriptRoot -Parent
$tools = Join-Path $repository '.tools'
New-Item -ItemType Directory -Force $tools | Out-Null

# Import Visual Studio's compiler and Windows SDK without requiring a developer shell.
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
$visualStudio = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (-not $visualStudio) { throw 'Install Visual Studio Build Tools with Desktop development with C++.' }
& "$visualStudio/Common7/Tools/Launch-VsDevShell.ps1" -Arch amd64 -HostArch amd64 -SkipAutomaticLocation
$env:RUSTUP_TOOLCHAIN = 'stable-x86_64-pc-windows-msvc'
rustc --version

# Bootstrap the exact vcpkg revision owning our GTK dependency manifest.
$manifest = Join-Path $repository 'build_support/windows'
$baseline = (Get-Content "$manifest/vcpkg.json" -Raw | ConvertFrom-Json).'builtin-baseline'
$vcpkg = Join-Path $tools 'vcpkg'
if (-not (Test-Path "$vcpkg/.git")) { git clone https://github.com/microsoft/vcpkg.git $vcpkg }
git -C $vcpkg checkout --detach $baseline
if (-not (Test-Path "$vcpkg/vcpkg.exe")) { & "$vcpkg/bootstrap-vcpkg.bat" -disableMetrics }
$env:VCPKG_ROOT = $vcpkg
$env:VCPKG_DOWNLOADS = Join-Path $tools 'downloads'
$env:VCPKG_DEFAULT_BINARY_CACHE = Join-Path $tools 'vcpkg-cache'
New-Item -ItemType Directory -Force $env:VCPKG_DEFAULT_BINARY_CACHE | Out-Null
$installed = Join-Path $tools 'native'
& "$vcpkg/vcpkg.exe" install "--x-manifest-root=$manifest" "--x-install-root=$installed" --triplet x64-windows-release --host-triplet x64-windows-release "--overlay-triplets=$manifest/triplets"
$env:CMUX_GTK_PREFIX = Join-Path $installed 'x64-windows-release'
$env:PKG_CONFIG_PATH = "$env:CMUX_GTK_PREFIX/lib/pkgconfig;$env:CMUX_GTK_PREFIX/share/pkgconfig"
$env:PKG_CONFIG = "$env:CMUX_GTK_PREFIX/tools/pkgconf/pkgconf.exe"
if (-not (Test-Path $env:PKG_CONFIG)) { throw 'vcpkg did not install pkgconf.exe.' }
# Zig invokes pkg-config by name; use the same native pkgconf executable as Cargo.
$nativeBin = Join-Path $tools 'bin'
New-Item -ItemType Directory -Force $nativeBin | Out-Null
Copy-Item $env:PKG_CONFIG "$nativeBin/pkg-config.exe" -Force
$env:PATH = "$nativeBin;$env:CMUX_GTK_PREFIX/bin;$env:PATH"
$env:LIB = "$env:CMUX_GTK_PREFIX/lib;$env:LIB"
$env:INCLUDE = "$env:CMUX_GTK_PREFIX/include;$env:INCLUDE"
$redist = Get-ChildItem "$visualStudio/VC/Redist/MSVC/*/x64/Microsoft.VC*.CRT" -Directory | Sort-Object FullName | Select-Object -Last 1
if (-not $redist) { throw 'Visual C++ redistributable DLLs were not found.' }
$env:CMUX_MSVC_RUNTIME_DIR = $redist.FullName

# Bindgen needs libclang, independently of the compiler used to build the application.
if (-not $env:LIBCLANG_PATH) {
    $env:LIBCLANG_PATH = "$env:ProgramFiles/LLVM/bin"
    if (-not (Test-Path "$env:LIBCLANG_PATH/libclang.dll")) {
        $clang = Join-Path $tools 'libclang'
        if (-not (Test-Path "$clang/clang/native/libclang.dll")) {
            python -m pip install --disable-pip-version-check --target $clang libclang==18.1.1
        }
        $env:LIBCLANG_PATH = "$clang/clang/native"
    }
}

# Download Ghostty's pinned Windows Zig package and verify its published checksum.
$version = [regex]::Match((Get-Content "$repository/ghostty/build.zig.zon" -Raw), 'minimum_zig_version = "([^"]+)"').Groups[1].Value
if (-not $version) { throw 'Initialize the pinned Ghostty submodule first.' }
$zigDirectory = Join-Path $tools "zig-x86_64-windows-$version"
if (-not (Test-Path "$zigDirectory/zig.exe")) {
    $index = Invoke-RestMethod https://ziglang.org/download/index.json
    $package = $index.$version.'x86_64-windows'
    if (-not $package) { throw "No Windows Zig package for $version" }
    Invoke-WebRequest $package.tarball -OutFile "$tools/zig.zip"
    if ((Get-FileHash "$tools/zig.zip" -Algorithm SHA256).Hash.ToLowerInvariant() -ne $package.shasum) { throw 'Zig checksum mismatch' }
    Expand-Archive "$tools/zig.zip" -DestinationPath $tools -Force
}
$env:PATH = "$zigDirectory;$env:PATH"
$env:ZIG_GLOBAL_CACHE_DIR = Join-Path $tools 'zig-cache'
New-Item -ItemType Directory -Force "$env:ZIG_GLOBAL_CACHE_DIR/tmp" | Out-Null

# Preserve the existing GTK icon theme without installing an MSYS2 environment.
$theme = "$env:CMUX_GTK_PREFIX/share/icons/Adwaita"
if (-not (Test-Path "$theme/index.theme")) {
    $archive = Join-Path $tools 'adwaita.tar.xz'
    Invoke-WebRequest https://download.gnome.org/sources/adwaita-icon-theme/49/adwaita-icon-theme-49.0.tar.xz -OutFile $archive
    if ((Get-FileHash $archive -Algorithm SHA256).Hash.ToLowerInvariant() -ne '65166461d1b278aa942f59aa8d0fccf1108d71c65f372c6266e172449791755c') { throw 'Adwaita checksum mismatch' }
    tar -xf $archive -C $tools
    New-Item -ItemType Directory -Force (Split-Path $theme -Parent) | Out-Null
    Copy-Item "$tools/adwaita-icon-theme-49.0/Adwaita" $theme -Recurse -Force
    Copy-Item "$tools/adwaita-icon-theme-49.0/index.theme" "$theme/index.theme"
    $license = "$env:CMUX_GTK_PREFIX/share/adwaita-icon-theme"
    New-Item -ItemType Directory -Force $license | Out-Null
    Copy-Item "$tools/adwaita-icon-theme-49.0/COPYING*" $license
}

if (-not $SkipBackend) {
    Push-Location "$repository/ghostty"
    try {
        # Keep Oniguruma private to Ghostty; its exported data cannot use GNU auto-import on MSVC.
        zig build -Dtarget=x86_64-windows-msvc -Dapp-runtime=none -Dembedded-app-thread-render=true -Dfont-backend=freetype_windows -Doptimize=ReleaseFast -Dcpu=baseline -Dsentry=false -Di18n=false -Demit-xcframework=false -fno-sys=oniguruma
        foreach ($artifact in @('lib/ghostty-internal.dll', 'lib/ghostty-internal.lib', 'include/ghostty.h')) {
            if (-not (Test-Path "zig-out/$artifact")) { throw "Missing Ghostty artifact: $artifact" }
        }
    } finally { Pop-Location }
}

# GitHub Actions steps run in separate processes; retain the same native environment.
if ($env:GITHUB_ENV) {
    foreach ($name in @('INCLUDE', 'LIB', 'LIBPATH', 'VCINSTALLDIR', 'VCToolsInstallDir', 'WindowsSdkDir', 'WindowsSDKVersion', 'UniversalCRTSdkDir', 'UCRTVersion', 'LIBCLANG_PATH', 'PKG_CONFIG_PATH', 'PKG_CONFIG', 'RUSTUP_TOOLCHAIN', 'CMUX_GTK_PREFIX', 'CMUX_MSVC_RUNTIME_DIR', 'ZIG_GLOBAL_CACHE_DIR')) {
        $value = [Environment]::GetEnvironmentVariable($name)
        if ($value) { Add-Content $env:GITHUB_ENV "$name=$value" }
    }
    $env:PATH -split ';' | Where-Object { $_ } | Select-Object -Unique | Add-Content $env:GITHUB_PATH
}
