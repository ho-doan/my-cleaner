$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$Repository = if ($env:CLEANRS_REPOSITORY) { $env:CLEANRS_REPOSITORY } else { "ho-doan/my-cleaner" }
$InstallDir = if ($env:CLEANRS_INSTALL_DIR) {
    $env:CLEANRS_INSTALL_DIR
} else {
    Join-Path $env:USERPROFILE ".cleanrs\bin"
}
$RetryAttempts = 6

function Get-ReleaseVersion {
    if ($env:CLEANRS_VERSION) {
        return $env:CLEANRS_VERSION.TrimStart("v")
    }

    $release = Invoke-RestMethod -UseBasicParsing -Uri "https://api.github.com/repos/$Repository/releases/latest"
    return $release.tag_name.TrimStart("v")
}

function Invoke-DownloadWithRetry([string] $Uri, [string] $OutputFile) {
    for ($attempt = 1; $attempt -le $RetryAttempts; $attempt++) {
        try {
            Write-Host "Downloading installer metadata ($attempt/$RetryAttempts)..."
            Invoke-WebRequest -UseBasicParsing -Headers @{ "Cache-Control" = "no-cache"; Pragma = "no-cache" } -Uri "$Uri&attempt=$attempt" -OutFile $OutputFile
            return
        } catch {
            if ($attempt -eq $RetryAttempts) {
                throw
            }
            $delay = [Math]::Min(2 * [Math]::Pow(2, $attempt - 1), 8)
            Start-Sleep -Seconds $delay
        }
    }
}

function Get-ExpectedChecksum([string] $Manifest, [string] $Archive) {
    $line = ($Manifest -split "`r?`n" | Where-Object {
        $_ -match "^([0-9a-fA-F]{64})\s+\*?$([regex]::Escape($Archive))$"
    } | Select-Object -First 1)
    if (-not $line) {
        throw "No verified checksum is available for $Archive yet. The release CDN may still be propagating."
    }
    return ([regex]::Match($line, "^[0-9a-fA-F]{64}")).Value.ToLowerInvariant()
}

$Version = Get-ReleaseVersion
if ([System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture -ne "X64") {
    throw "Windows installer currently supports only x86_64 (AMD64)."
}

$Target = "x86_64-pc-windows-msvc"
$Archive = "cleanrs-v$Version-$Target.zip"
$BaseUrl = "https://github.com/$Repository/releases/download/v$Version"
$TempDir = Join-Path ([System.IO.Path]::GetTempPath()) ("cleanrs-install-" + [System.IO.Path]::GetRandomFileName())
$ArchivePath = Join-Path $TempDir $Archive
$ManifestPath = Join-Path $TempDir "checksums.txt"

New-Item -ItemType Directory -Path $TempDir -Force | Out-Null
try {
    $checksumsUrl = "$BaseUrl/checksums.txt?version=$Version"
    Invoke-DownloadWithRetry $checksumsUrl $ManifestPath
    $manifest = Get-Content -Raw -Path $ManifestPath
    $expected = Get-ExpectedChecksum $manifest $Archive

    Write-Host "Downloading cleanrs $Version for $Target..."
    Invoke-DownloadWithRetry "$BaseUrl/$Archive?version=$Version" $ArchivePath
    $actual = (Get-FileHash -Algorithm SHA256 -Path $ArchivePath).Hash.ToLowerInvariant()
    if ($actual -ne $expected) {
        throw "Checksum verification failed. Expected $expected, got $actual."
    }

    Expand-Archive -Path $ArchivePath -DestinationPath $TempDir -Force
    $binary = Join-Path $TempDir "cleanrs.exe"
    if (-not (Test-Path -Path $binary -PathType Leaf)) {
        throw "The release archive does not contain cleanrs.exe."
    }

    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    Copy-Item -Path $binary -Destination (Join-Path $InstallDir "cleanrs.exe") -Force

    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    $pathEntries = if ($userPath) { $userPath -split ";" } else { @() }
    if ($pathEntries -notcontains $InstallDir) {
        $newPath = (($pathEntries + $InstallDir) | Where-Object { $_ }) -join ";"
        [Environment]::SetEnvironmentVariable("Path", $newPath, "User")
        Write-Host "Added $InstallDir to the user PATH. Open a new terminal to use cleanrs."
    }
    Write-Host "Installed cleanrs $Version to $(Join-Path $InstallDir 'cleanrs.exe')"
} finally {
    if (Test-Path $TempDir) {
        Remove-Item -Recurse -Force $TempDir
    }
}
