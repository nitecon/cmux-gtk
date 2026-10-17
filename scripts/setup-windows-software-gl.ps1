# Install a checksum-pinned MSVC Mesa driver for disposable hosted-runner smoke tests.
# This driver is never copied into the distributed application bundle.
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
$tools = Join-Path (Split-Path $PSScriptRoot -Parent) '.tools'
New-Item -ItemType Directory -Force $tools | Out-Null
$archive = Join-Path $tools 'mesa-msvc.7z'
Invoke-WebRequest https://github.com/pal1000/mesa-dist-win/releases/download/26.2.4/mesa3d-26.2.4-release-msvc.7z -OutFile $archive
if ((Get-FileHash $archive -Algorithm SHA256).Hash.ToLowerInvariant() -ne '351fc8c8b695878ffb3eaa044b3ead08672a48b1a045e3c3e3975811df0f6695') { throw 'Mesa checksum mismatch' }
& "$env:ProgramFiles/7-Zip/7z.exe" x $archive "-o$tools/mesa" -y
if (-not (Test-Path "$tools/mesa/x64/opengl32.dll")) { throw 'Mesa archive layout unrecognized' }
$env:CMUX_SOFTWARE_GL_DIR = "$tools/mesa/x64"
if ($env:GITHUB_ENV) { Add-Content $env:GITHUB_ENV "CMUX_SOFTWARE_GL_DIR=$env:CMUX_SOFTWARE_GL_DIR" }
