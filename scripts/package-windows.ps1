#!/usr/bin/env pwsh
# Fail-closed Windows x64 Technical Preview packaging (T-289).
# The unsigned portable ZIP is always produced. An optional signed MSIX is emitted only
# when NEOFLASH_WINDOWS_CERT_THUMBPRINT is configured on the runner.

param(
    [ValidateSet("release", "debug")]
    [string]$Profile = "release"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
if ($PSVersionTable.PSVersion.Major -ge 7) {
    $PSNativeCommandUseErrorActionPreference = $true
}

$ProjectRoot = Split-Path -Parent $PSScriptRoot
$Version = "0.1.0-beta.1"
$PackageName = "NeoFlash-$Version-windows-x64-preview"
$DistDir = Join-Path $ProjectRoot "dist"
$WorkDir = Join-Path $ProjectRoot "target/package-windows"
$StageDir = Join-Path $WorkDir $PackageName
$RustLicenses = Join-Path $WorkDir "rust-licenses"
$ArchivePath = Join-Path $DistDir "$PackageName.zip"
$SourceOutput = Join-Path $DistDir "NeoFlash-$Version-portable-media-corresponding-source.tar.xz"
$ChecksumFile = Join-Path $DistDir "NeoFlash-$Version-windows-x64-SHA256SUMS.txt"
$MsixPath = Join-Path $DistDir "$PackageName.msix"
$PackageComplete = $false

Push-Location $ProjectRoot
try {
    $Dirty = @(git status --porcelain=v1 --untracked-files=all)
    if ($LASTEXITCODE -ne 0) {
        throw "Could not inspect the Git worktree before packaging."
    }
    if ($Dirty.Count -ne 0) {
        throw "Refusing to package source files that do not exactly match HEAD."
    }
    $IsShallow = (git rev-parse --is-shallow-repository).Trim()
    if ($LASTEXITCODE -ne 0 -or $IsShallow -eq "true") {
        throw "Refusing to package a shallow checkout: the real build number is unavailable."
    }

    $Commit = (git rev-parse --verify HEAD).Trim()
    if ($LASTEXITCODE -ne 0) { throw "Could not resolve the Git commit." }
    $BuildNumber = [int]((git rev-list --count HEAD).Trim())
    if ($LASTEXITCODE -ne 0) { throw "Could not resolve the Git build number." }
    $Python = if (Get-Command python3 -ErrorAction SilentlyContinue) { "python3" } else { "python" }
    New-Item -ItemType Directory -Force -Path $DistDir, $WorkDir | Out-Null
    foreach ($OldOutput in @($ArchivePath, $SourceOutput, $ChecksumFile, $MsixPath)) {
        Remove-Item -Force -ErrorAction SilentlyContinue $OldOutput
    }
    Remove-Item -Recurse -Force -ErrorAction SilentlyContinue $StageDir, $RustLicenses

    rustup target add wasm32-unknown-unknown
    if ($LASTEXITCODE -ne 0) { throw "Could not install the wasm32 Rust target." }
    if (-not (Get-Command wasm-bindgen -ErrorAction SilentlyContinue) -or ((wasm-bindgen --version) -ne "wasm-bindgen 0.2.129")) {
        cargo install wasm-bindgen-cli --version 0.2.129 --locked
        if ($LASTEXITCODE -ne 0) { throw "Could not install pinned wasm-bindgen-cli." }
    }
    cargo build --locked --release -p neoflash-web --target wasm32-unknown-unknown
    if ($LASTEXITCODE -ne 0) { throw "Web runtime build failed." }
    wasm-bindgen --target no-modules --no-modules-global NeoFlashWasm --out-name neoflash_web --out-dir target/web-runtime target/wasm32-unknown-unknown/release/neoflash_web.wasm
    if ($LASTEXITCODE -ne 0) { throw "wasm-bindgen generation failed." }

    if ($Profile -eq "release") {
        cargo build --locked --release -p neoflash-editor
        if ($LASTEXITCODE -ne 0) { throw "Release editor build failed." }
    } else {
        cargo build --locked -p neoflash-editor
        if ($LASTEXITCODE -ne 0) { throw "Debug editor build failed." }
    }
    $ExeSource = Join-Path $ProjectRoot "target/$Profile/neoflash-editor.exe"
    if (-not (Test-Path -PathType Leaf $ExeSource)) {
        throw "Expected Windows executable not found at $ExeSource"
    }

    & $Python scripts/stage-rust-dependency-licenses.py `
        --project-root $ProjectRoot `
        --package neoflash-editor `
        --dest $RustLicenses `
        --license-cache (Join-Path $ProjectRoot "target/rust-license-text-cache") `
        --download-license-texts
    if ($LASTEXITCODE -ne 0) { throw "Rust dependency license staging failed." }

    $MediaRoot = if ($env:NEOFLASH_WINDOWS_MEDIA_ROOT) {
        $env:NEOFLASH_WINDOWS_MEDIA_ROOT
    } else {
        Join-Path $ProjectRoot "target/bundled-media/windows-x64"
    }
    $MediaSourceArchive = if ($env:NEOFLASH_WINDOWS_MEDIA_SOURCE_ARCHIVE) {
        $env:NEOFLASH_WINDOWS_MEDIA_SOURCE_ARCHIVE
    } else {
        Join-Path $ProjectRoot "target/bundled-media/NeoFlash-$Version-portable-media-corresponding-source.tar.xz"
    }

    $PortableArgs = @(
        "--platform", "windows-x64",
        "--project-root", $ProjectRoot,
        "--version", $Version,
        "--profile", $Profile,
        "--commit", $Commit,
        "--build-number", $BuildNumber,
        "--executable", $ExeSource,
        "--runtime-dir", (Join-Path $ProjectRoot "target/web-runtime"),
        "--media-root", $MediaRoot,
        "--media-source-archive", $MediaSourceArchive,
        "--rust-licenses", $RustLicenses,
        "--stage-dir", $StageDir,
        "--package-name", $PackageName,
        "--archive", $ArchivePath,
        "--source-output", $SourceOutput,
        "--checksum-file", $ChecksumFile
    )
    & $Python scripts/package-portable.py stage @PortableArgs
    if ($LASTEXITCODE -ne 0) { throw "Portable Windows staging failed." }

    $IconSourceDir = Join-Path $ProjectRoot "crates/neoflash-editor/assets/app-icon/windows"
    $AssetsStageDir = Join-Path $StageDir "Assets"
    New-Item -ItemType Directory -Force -Path $AssetsStageDir | Out-Null
    Copy-Item -Force (Join-Path $IconSourceDir "NeoFlash.ico") (Join-Path $StageDir "NeoFlash.ico")
    foreach ($Logo in @("Square44x44Logo.png", "Square150x150Logo.png", "StoreLogo.png")) {
        Copy-Item -Force (Join-Path $IconSourceDir $Logo) (Join-Path $AssetsStageDir $Logo)
    }

    $CertThumbprint = $env:NEOFLASH_WINDOWS_CERT_THUMBPRINT
    if (-not [string]::IsNullOrWhiteSpace($CertThumbprint)) {
        # Keep the pinned upstream FFmpeg/FFprobe bytes unchanged. Only NeoFlash's own
        # executable is signed; the final MSIX receives its own outer signature below.
        $EditorExe = Join-Path $StageDir "neoflash-editor.exe"
        signtool.exe sign /fd SHA256 /sha1 $CertThumbprint /tr http://timestamp.digicert.com /td SHA256 $EditorExe
        if ($LASTEXITCODE -ne 0) { throw "Authenticode signing failed for $EditorExe" }
    }

    & $Python scripts/package-portable.py finalize @PortableArgs
    if ($LASTEXITCODE -ne 0) { throw "Portable Windows finalization failed." }

    if (-not [string]::IsNullOrWhiteSpace($CertThumbprint)) {
        $MsixStage = Join-Path $WorkDir "msix"
        Remove-Item -Recurse -Force -ErrorAction SilentlyContinue $MsixStage
        New-Item -ItemType Directory -Force -Path $MsixStage | Out-Null
        Copy-Item -Recurse -Force (Join-Path $StageDir "*") $MsixStage
        Copy-Item -Force (Join-Path $ProjectRoot "scripts/windows-appxmanifest.xml") (Join-Path $MsixStage "AppxManifest.xml")
        $Certificate = Get-Item "Cert:\CurrentUser\My\$CertThumbprint" -ErrorAction Stop
        [xml]$ManifestXml = Get-Content -Raw (Join-Path $MsixStage "AppxManifest.xml")
        $ManifestXml.Package.Identity.Publisher = $Certificate.Subject
        $ManifestXml.Save((Join-Path $MsixStage "AppxManifest.xml"))
        makeappx.exe pack /o /d $MsixStage /p $MsixPath
        if ($LASTEXITCODE -ne 0) { throw "MSIX packaging failed." }
        signtool.exe sign /fd SHA256 /sha1 $CertThumbprint /tr http://timestamp.digicert.com /td SHA256 $MsixPath
        if ($LASTEXITCODE -ne 0) { throw "MSIX signing failed." }
        $MsixHash = (Get-FileHash -Algorithm SHA256 -Path $MsixPath).Hash.ToLowerInvariant()
        Add-Content -Encoding UTF8 $ChecksumFile "$MsixHash  $(Split-Path -Leaf $MsixPath)"
    }

    $PackageComplete = $true
    Write-Output $ArchivePath
} finally {
    if (-not $PackageComplete) {
        foreach ($Incomplete in @($ArchivePath, $SourceOutput, $ChecksumFile, $MsixPath)) {
            Remove-Item -Force -ErrorAction SilentlyContinue $Incomplete
        }
    }
    Pop-Location
}
