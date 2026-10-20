<#
.SYNOPSIS
  Builds and packages OxyPad for Windows.

.DESCRIPTION
  Produces, in $env:DIST (default: dist), each with a .sha256 next to it:
    OxyPad-<version>-windows-<arch>.msi            per-machine installer (WiX v5)
    OxyPad-<version>-windows-<arch>-portable.zip   OxyPad.exe, no installation

  OxyPad.exe links the C runtime statically (.cargo/config.toml), so neither
  needs the Visual C++ Redistributable. The files are not signed yet.

  Needs: Rust (MSVC toolchain and the target) and WiX v5:
    dotnet tool install --global wix --version 5.0.2

.EXAMPLE
  pwsh packaging/windows/package.ps1 -Arch x64
  pwsh packaging/windows/package.ps1 -Arch arm64     # cross-compiled; needs the MSVC ARM64 build tools
#>
param(
  [ValidateSet('x64', 'arm64')] [string] $Arch = 'x64',
  [switch] $SkipBuild
)
$ErrorActionPreference = 'Stop'
$Root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path

function Invoke-Native([string] $What, [scriptblock] $Block) {
  Write-Output "==> $What"
  & $Block
  if ($LASTEXITCODE -ne 0) { throw "$What failed with exit code $LASTEXITCODE" }
}

function Write-Sha256([string] $File) {
  $hash = (Get-FileHash $File -Algorithm SHA256).Hash.ToLower()
  "$hash  $(Split-Path $File -Leaf)`n" | Out-File -Encoding ascii -NoNewline "$File.sha256"
}

# The version lives in one place: [workspace.package] version in Cargo.toml.
$Version = $env:OXYPAD_VERSION
if (-not $Version) {
  $inPackage = $false
  foreach ($line in Get-Content (Join-Path $Root 'Cargo.toml')) {
    if ($line -match '^\s*\[') { $inPackage = ($line.Trim() -eq '[workspace.package]'); continue }
    if ($inPackage -and $line -match '^\s*version\s*=\s*"([^"]+)"') { $Version = $Matches[1]; break }
  }
}
if (-not $Version) { throw 'could not read [workspace.package] version from Cargo.toml' }
# MSI versions are numbers only: 0.2.0-beta.1 installs as 0.2.0.
$MsiVersion = ($Version -split '[-+]')[0]

$Target = switch ($Arch) { 'x64' { 'x86_64-pc-windows-msvc' } 'arm64' { 'aarch64-pc-windows-msvc' } }
$Dist = if ($env:DIST) { $env:DIST } else { Join-Path $Root 'dist' }
$TargetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $Root 'target' }
New-Item -ItemType Directory -Force -Path $Dist | Out-Null
$Exe = Join-Path $TargetDir "$Target\release\oxypad.exe"

Write-Output "OxyPad $Version for Windows $Arch ($Target)"

if (-not $SkipBuild) {
  Push-Location $Root
  try {
    Invoke-Native 'cargo build' { cargo build --release --locked -p oxypad --target $Target }
  } finally {
    Pop-Location
  }
}
if (-not (Test-Path $Exe)) { throw "$Exe not found; build it or drop -SkipBuild" }

# The executable must be for the architecture of the package.
$bytes = [System.IO.File]::ReadAllBytes($Exe)
$machine = [BitConverter]::ToUInt16($bytes, [BitConverter]::ToInt32($bytes, 0x3C) + 4)
$expected = switch ($Arch) { 'x64' { 0x8664 } 'arm64' { 0xAA64 } }
if ($machine -ne $expected) { throw "oxypad.exe is for machine 0x$('{0:X}' -f $machine), not $Arch" }

# ---- portable zip ----------------------------------------------------------------------------
$name = "OxyPad-$Version-windows-$Arch-portable"
$work = Join-Path $TargetDir "windows-package\$Arch"
$folder = Join-Path $work 'OxyPad'
Remove-Item -Recurse -Force -ErrorAction SilentlyContinue $work
New-Item -ItemType Directory -Force $folder | Out-Null
Copy-Item $Exe (Join-Path $folder 'OxyPad.exe')
Copy-Item (Join-Path $Root 'LICENSE') (Join-Path $folder 'LICENSE.txt')
$zip = Join-Path $Dist "$name.zip"
Remove-Item -ErrorAction SilentlyContinue $zip
Compress-Archive -Path $folder -DestinationPath $zip
Write-Sha256 $zip
Write-Output "wrote $zip"

# ---- MSI -------------------------------------------------------------------------------------
$wix = Get-Command wix -ErrorAction SilentlyContinue
$wix = if ($wix) { $wix.Source } else { Join-Path $env:USERPROFILE '.dotnet\tools\wix.exe' }
if (-not (Test-Path $wix)) { throw 'WiX not found: dotnet tool install --global wix --version 5.0.2' }
$msi = Join-Path $Dist "OxyPad-$Version-windows-$Arch.msi"
Invoke-Native 'wix build' {
  & $wix build (Join-Path $Root 'packaging\windows\OxyPad.wxs') (Join-Path $Root 'packaging\windows\installer-ui.wxs') `
    -arch $Arch `
    -d "Version=$MsiVersion" `
    -d "ExeFile=$Exe" `
    -d "IconFile=$(Join-Path $Root 'oxypad-icons\oxypad.ico')" `
    -d "LicenseFile=$(Join-Path $Root 'LICENSE')" `
    -o $msi
}
Remove-Item -ErrorAction SilentlyContinue (Join-Path $Dist '*.wixpdb')
Write-Sha256 $msi
Write-Output "wrote $msi"

Get-ChildItem $Dist | Format-Table Name, Length
