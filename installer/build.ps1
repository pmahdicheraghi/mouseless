# Builds the release binary and the installer: target\installer\mouseless-setup-<version>.exe
# Requires Inno Setup 6 (install it with: winget install JRSoftware.InnoSetup)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
Push-Location $root
try {
    cargo build --release
    if ($LASTEXITCODE) { throw "cargo build failed" }

    $version = (Select-String -Path Cargo.toml -Pattern '^version\s*=\s*"(.+)"').Matches[0].Groups[1].Value

    $iscc = (Get-Command iscc -ErrorAction SilentlyContinue).Source
    if (-not $iscc) {
        $iscc = @(
            "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
            "$env:ProgramFiles\Inno Setup 6\ISCC.exe",
            "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe"
        ) | Where-Object { Test-Path $_ } | Select-Object -First 1
    }
    if (-not $iscc) { throw "Inno Setup 6 was not found, install it with: winget install JRSoftware.InnoSetup" }

    & $iscc "/DAppVersion=$version" "installer\mouseless.iss"
    if ($LASTEXITCODE) { throw "building the installer failed" }
    Write-Host "Installer: $root\target\installer\mouseless-setup-$version.exe"
}
finally {
    Pop-Location
}
