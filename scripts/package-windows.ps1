#!/usr/bin/env pwsh
# Fail-closed Windows x64 Technical Preview packaging (T-289 / T-501).
# Produces the Windows ZIP (including per-user Install-NeoFlash.ps1 / Uninstall-NeoFlash.ps1 and
# BUNDLE-PROVENANCE.json), with optional fail-closed Authenticode signing (-Sign,
# NEOFLASH_WINDOWS_CERT_SHA1 / NEOFLASH_WINDOWS_CERT_THUMBPRINT, or NEOFLASH_WINDOWS_PFX_PATH).

param(
    [ValidateSet("release", "debug")]
    [string]$Profile = "release",
    [switch]$Sign
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
if ($PSVersionTable.PSVersion.Major -ge 7) {
    $PSNativeCommandUseErrorActionPreference = $true
}

function Redact-SigningSecrets {
    param([string]$Text)
    if ([string]::IsNullOrEmpty($Text)) {
        return ""
    }
    $Redacted = $Text
    foreach ($Secret in @($env:NEOFLASH_WINDOWS_PFX_PASSWORD, $env:NEOFLASH_WINDOWS_PFX_PATH)) {
        if (-not [string]::IsNullOrEmpty($Secret)) {
            $Redacted = $Redacted.Replace($Secret, "[REDACTED]")
        }
    }
    return $Redacted
}

function Resolve-SignToolExe {
    if (-not [string]::IsNullOrWhiteSpace($env:NEOFLASH_SIGNTOOL)) {
        if (-not (Test-Path -LiteralPath $env:NEOFLASH_SIGNTOOL -PathType Leaf)) {
            throw "Configured NEOFLASH_SIGNTOOL does not exist: $($env:NEOFLASH_SIGNTOOL)"
        }
        return (Resolve-Path -LiteralPath $env:NEOFLASH_SIGNTOOL).Path
    }
    $InPath = Get-Command signtool.exe -ErrorAction SilentlyContinue
    if ($null -ne $InPath -and -not [string]::IsNullOrWhiteSpace($InPath.Source)) {
        return $InPath.Source
    }
    $SdkRoots = @()
    foreach ($ProgRoot in @(${env:ProgramFiles(x86)}, $env:ProgramFiles)) {
        if (-not [string]::IsNullOrWhiteSpace($ProgRoot)) {
            $KitBin = Join-Path $ProgRoot "Windows Kits\10\bin"
            if (Test-Path -LiteralPath $KitBin -PathType Container) {
                $SdkRoots += $KitBin
            }
        }
    }
    foreach ($KitBin in $SdkRoots) {
        $Candidates = @(
            Get-ChildItem -LiteralPath $KitBin -Directory -ErrorAction SilentlyContinue |
                Where-Object { $_.Name -match '^\d+\.\d+\.\d+\.\d+$' } |
                Sort-Object { [version]$_.Name } -Descending
        )
        foreach ($VerDir in $Candidates) {
            $Exe = Join-Path $VerDir.FullName "x64\signtool.exe"
            if (Test-Path -LiteralPath $Exe -PathType Leaf) {
                return $Exe
            }
        }
    }
    throw "Authenticode signing requires signtool.exe from the Windows 10/11 SDK (or NEOFLASH_SIGNTOOL)."
}

function Invoke-SignAndVerifyBinary {
    param(
        [Parameter(Mandatory = $true)][string]$SignTool,
        [Parameter(Mandatory = $true)][string]$TargetFile,
        [Parameter(Mandatory = $true)][string]$TimestampUrl,
        [string]$CertSha1 = "",
        [string]$PfxPath = ""
    )
    $SignArgs = @(
        "sign",
        "/fd", "SHA256",
        "/tr", $TimestampUrl,
        "/td", "SHA256"
    )
    if (-not [string]::IsNullOrWhiteSpace($CertSha1)) {
        $SignArgs += @("/sha1", $CertSha1)
    } elseif (-not [string]::IsNullOrWhiteSpace($PfxPath)) {
        $SignArgs += @("/f", $PfxPath)
        if (-not [string]::IsNullOrEmpty($env:NEOFLASH_WINDOWS_PFX_PASSWORD)) {
            $SignArgs += @("/p", $env:NEOFLASH_WINDOWS_PFX_PASSWORD)
        }
    } else {
        throw "Internal error: neither CertSha1 nor PfxPath provided to Invoke-SignAndVerifyBinary."
    }
    $SignArgs += @($TargetFile)

    $SavedEap = $ErrorActionPreference
    try {
        $ErrorActionPreference = "Continue"
        $SignOutput = & $SignTool @SignArgs 2>&1 | Out-String
        $SignExit = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $SavedEap
    }
    if ($SignExit -ne 0) {
        $SafeOut = Redact-SigningSecrets $SignOutput
        throw "Authenticode signing failed for $(Split-Path -Leaf $TargetFile) (exit $SignExit): $($SafeOut.Trim())"
    }

    $SavedEap = $ErrorActionPreference
    try {
        $ErrorActionPreference = "Continue"
        $VerifyOutput = & $SignTool verify /pa /v $TargetFile 2>&1 | Out-String
        $VerifyExit = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $SavedEap
    }
    if ($VerifyExit -ne 0) {
        $SafeVerify = Redact-SigningSecrets $VerifyOutput
        throw "Authenticode verification (signtool verify /pa /v) failed for $(Split-Path -Leaf $TargetFile) (exit $VerifyExit): $($SafeVerify.Trim())"
    }
}

$ProjectRoot = Split-Path -Parent $PSScriptRoot
$Version = "0.1.0-beta.1"
$PackageName = "NeoFlash-$Version-windows-x64-preview"
$DistDir = Join-Path $ProjectRoot "dist"
$WorkDir = Join-Path $ProjectRoot "target/package-windows"
$StageDir = Join-Path $WorkDir $PackageName
$RustLicenses = Join-Path $WorkDir "rust-licenses"
$PreSignHashesFile = Join-Path $WorkDir "pre-sign-hashes.json"
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

    # Validate Authenticode configuration fail-closed up front before expensive compilation.
    $SignFlagPy = if ($Sign.IsPresent) { "True" } else { "False" }
    $AuthJson = & $Python -c "import importlib.util, json, os, sys; spec = importlib.util.spec_from_file_location('pp', 'scripts/package-portable.py'); m = importlib.util.module_from_spec(spec); spec.loader.exec_module(m); cfg = m.resolve_windows_authenticode_config(os.environ, sign_requested=$SignFlagPy); cfg.pop('pfx_path', None); print(json.dumps(cfg))"
    if ($LASTEXITCODE -ne 0) {
        throw "Invalid Windows Authenticode signing configuration."
    }
    $AuthConfig = $AuthJson | ConvertFrom-Json
    $SignToolExe = $null
    if ($AuthConfig.enabled) {
        $SignToolExe = Resolve-SignToolExe
    }

    New-Item -ItemType Directory -Force -Path $DistDir, $WorkDir | Out-Null
    foreach ($OldOutput in @($ArchivePath, $SourceOutput, $ChecksumFile, $MsixPath, $PreSignHashesFile)) {
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
    Copy-Item -Force (Join-Path $ProjectRoot "web/as-intrinsics.js") (Join-Path $ProjectRoot "target/web-runtime/as-intrinsics.js")

    $SavedRustFlags = $env:RUSTFLAGS
    $HadRustFlags = Test-Path Env:\RUSTFLAGS
    $SavedEncodedRustFlags = $env:CARGO_ENCODED_RUSTFLAGS
    $HadEncodedRustFlags = Test-Path Env:\CARGO_ENCODED_RUSTFLAGS
    try {
        # Link the MSVC CRT statically for the native Windows executable so clean Windows 10/11
        # installations without vcruntime140.dll launch without 0xC0000135 (STATUS_DLL_NOT_FOUND).
        # Keep this scoped strictly to the native Windows build (never wasm32 or .cargo/config.toml).
        if ($HadEncodedRustFlags -and -not [string]::IsNullOrEmpty($SavedEncodedRustFlags)) {
            if ($SavedEncodedRustFlags -notmatch '(^|\x1f|,)\+crt-static($|\x1f|,)') {
                $env:CARGO_ENCODED_RUSTFLAGS = "$SavedEncodedRustFlags`u{001f}-C`u{001f}target-feature=+crt-static"
            }
        } else {
            Remove-Item Env:\CARGO_ENCODED_RUSTFLAGS -ErrorAction SilentlyContinue
            if ([string]::IsNullOrWhiteSpace($SavedRustFlags)) {
                $env:RUSTFLAGS = "-C target-feature=+crt-static"
            } elseif ($SavedRustFlags -notmatch '(^|\s|,)\+crt-static($|\s|,)') {
                $env:RUSTFLAGS = "$SavedRustFlags -C target-feature=+crt-static"
            }
        }
        if ($Profile -eq "release") {
            cargo build --locked --release -p neoflash-editor
            if ($LASTEXITCODE -ne 0) { throw "Release editor build failed." }
        } else {
            cargo build --locked -p neoflash-editor
            if ($LASTEXITCODE -ne 0) { throw "Debug editor build failed." }
        }
    } finally {
        if ($HadRustFlags) {
            $env:RUSTFLAGS = $SavedRustFlags
        } else {
            Remove-Item Env:\RUSTFLAGS -ErrorAction SilentlyContinue
        }
        if ($HadEncodedRustFlags) {
            $env:CARGO_ENCODED_RUSTFLAGS = $SavedEncodedRustFlags
        } else {
            Remove-Item Env:\CARGO_ENCODED_RUSTFLAGS -ErrorAction SilentlyContinue
        }
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

    if ($AuthConfig.enabled) {
        $CertSha1 = if ($null -ne $AuthConfig.cert_sha1) { [string]$AuthConfig.cert_sha1 } else { "" }
        $PfxPath = if ($AuthConfig.mode -eq "pfx_file") { (Resolve-Path -LiteralPath $env:NEOFLASH_WINDOWS_PFX_PATH).Path } else { "" }
        $TimestampUrl = [string]$AuthConfig.timestamp_url

        $SignTargets = @((Join-Path $StageDir "neoflash-editor.exe"))
        foreach ($SubDir in @("media\bin", "media\lib")) {
            $CandidateDir = Join-Path $StageDir $SubDir
            if (Test-Path -LiteralPath $CandidateDir -PathType Container) {
                $FoundMediaBins = @(
                    Get-ChildItem -LiteralPath $CandidateDir -Recurse -File |
                        Where-Object { $_.Extension -in @(".exe", ".dll") } |
                        Sort-Object FullName
                )
                foreach ($Item in $FoundMediaBins) {
                    $SignTargets += $Item.FullName
                }
            }
        }

        $PreHashes = [ordered]@{}
        foreach ($TargetFile in $SignTargets) {
            $Rel = [System.IO.Path]::GetRelativePath($StageDir, $TargetFile).Replace('\', '/')
            $PreHashes[$Rel] = (Get-FileHash -Algorithm SHA256 -LiteralPath $TargetFile).Hash.ToLowerInvariant()
        }
        ($PreHashes | ConvertTo-Json -Depth 4) | Set-Content -LiteralPath $PreSignHashesFile -Encoding UTF8

        foreach ($TargetFile in $SignTargets) {
            Invoke-SignAndVerifyBinary `
                -SignTool $SignToolExe `
                -TargetFile $TargetFile `
                -TimestampUrl $TimestampUrl `
                -CertSha1 $CertSha1 `
                -PfxPath $PfxPath
        }

        $RecordArgs = @(
            "record-signing",
            "--signing-credential-source", [string]$AuthConfig.mode,
            "--signing-timestamp-url", $TimestampUrl,
            "--signing-pre-hashes", $PreSignHashesFile
        )
        if (-not [string]::IsNullOrWhiteSpace($CertSha1)) {
            $RecordArgs += @("--signing-cert-sha1", $CertSha1)
        }
        & $Python scripts/package-portable.py @RecordArgs @PortableArgs
        if ($LASTEXITCODE -ne 0) {
            throw "Recording Windows Authenticode provenance failed."
        }
        Remove-Item -LiteralPath $PreSignHashesFile -Force -ErrorAction SilentlyContinue
    }

    & $Python scripts/package-portable.py finalize @PortableArgs
    if ($LASTEXITCODE -ne 0) { throw "Portable Windows finalization failed." }

    if ($AuthConfig.enabled -and $AuthConfig.mode -eq "cert_store_sha1" -and (Get-Command makeappx.exe -ErrorAction SilentlyContinue)) {
        $CertThumbprint = [string]$AuthConfig.cert_sha1
        $TimestampUrl = [string]$AuthConfig.timestamp_url
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
        Invoke-SignAndVerifyBinary `
            -SignTool $SignToolExe `
            -TargetFile $MsixPath `
            -TimestampUrl $TimestampUrl `
            -CertSha1 $CertThumbprint
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
