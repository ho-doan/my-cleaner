$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

# Windows PowerShell 5.1 may default to TLS 1.0/1.1, which GitHub rejects.
try {
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
} catch {
    # Keep the installer usable on PowerShell implementations that do not expose
    # ServicePointManager; curl.exe still negotiates TLS independently.
}

$Repository = if ($env:CLEANRS_REPOSITORY) { $env:CLEANRS_REPOSITORY } else { "ho-doan/my-cleaner" }
$InstallDir = if ($env:CLEANRS_INSTALL_DIR) {
    $env:CLEANRS_INSTALL_DIR
} else {
    Join-Path $env:USERPROFILE ".cleanrs\bin"
}
$RetryAttempts = 6

function Get-CurlPath {
    $curlCommand = Get-Command curl.exe -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($null -eq $curlCommand) {
        return $null
    }
    return $curlCommand.Source
}

function Get-AttemptUri([string] $Uri, [int] $Attempt) {
    $separator = if ($Uri.Contains("?")) { "&" } else { "?" }
    return "$Uri$separator" + "attempt=$Attempt"
}

function Invoke-TextWithRetry([string] $Uri, [string] $Description) {
    $lastError = "unknown error"
    for ($attempt = 1; $attempt -le $RetryAttempts; $attempt++) {
        try {
            Write-Host "$Description ($attempt/$RetryAttempts)..."
            $requestUri = Get-AttemptUri $Uri $attempt
            $curlPath = Get-CurlPath
            if ($null -ne $curlPath) {
                $curlArguments = @(
                    "--fail", "--silent", "--show-error", "--location",
                    "--retry", "3", "--retry-delay", "2",
                    "--connect-timeout", "15", "--max-time", "120",
                    "-H", "Cache-Control: no-cache", "-H", "Pragma: no-cache",
                    $requestUri
                )
                $response = & $curlPath @curlArguments
                if ($LASTEXITCODE -ne 0) {
                    throw "curl.exe exited with status $LASTEXITCODE."
                }
                return ($response -join [Environment]::NewLine)
            }

            return (Invoke-WebRequest -UseBasicParsing -Headers @{ "Cache-Control" = "no-cache"; Pragma = "no-cache" } -TimeoutSec 120 -Uri $requestUri).Content
        } catch {
            $lastError = $_.Exception.Message
            if ($attempt -eq $RetryAttempts) {
                break
            }
            $delay = [Math]::Min(2 * [Math]::Pow(2, $attempt - 1), 8)
            Write-Warning "$Description failed: $lastError. Retrying in $delay seconds."
            Start-Sleep -Seconds $delay
        }
    }
    throw "$Description failed after $RetryAttempts attempts. Last error: $lastError"
}

function Get-ReleaseVersion {
    if ($env:CLEANRS_VERSION) {
        return $env:CLEANRS_VERSION.TrimStart("v")
    }

    $releaseJson = Invoke-TextWithRetry "https://api.github.com/repos/$Repository/releases/latest" "Checking latest release"
    $release = $releaseJson | ConvertFrom-Json
    if ($null -eq $release -or [string]::IsNullOrWhiteSpace([string]$release.tag_name)) {
        throw "GitHub did not return a latest release tag for $Repository."
    }
    return ([string]$release.tag_name).TrimStart("v")
}

function Invoke-DownloadWithRetry([string] $Uri, [string] $OutputFile, [string] $Description) {
    $lastError = "unknown error"
    for ($attempt = 1; $attempt -le $RetryAttempts; $attempt++) {
        try {
            Write-Host "$Description ($attempt/$RetryAttempts)..."
            $requestUri = Get-AttemptUri $Uri $attempt
            $curlPath = Get-CurlPath
            if ($null -ne $curlPath) {
                $curlArguments = @(
                    "--fail", "--silent", "--show-error", "--location",
                    "--retry", "3", "--retry-delay", "2",
                    "--connect-timeout", "15", "--max-time", "300",
                    "-H", "Cache-Control: no-cache", "-H", "Pragma: no-cache",
                    "--output", $OutputFile, $requestUri
                )
                & $curlPath @curlArguments
                if ($LASTEXITCODE -ne 0) {
                    throw "curl.exe exited with status $LASTEXITCODE."
                }
            } else {
                Invoke-WebRequest -UseBasicParsing -Headers @{ "Cache-Control" = "no-cache"; Pragma = "no-cache" } -TimeoutSec 300 -Uri $requestUri -OutFile $OutputFile
            }
            return
        } catch {
            $lastError = $_.Exception.Message
            if ($attempt -eq $RetryAttempts) {
                break
            }
            $delay = [Math]::Min(2 * [Math]::Pow(2, $attempt - 1), 8)
            Write-Warning "$Description failed: $lastError. Retrying in $delay seconds."
            Start-Sleep -Seconds $delay
        }
    }
    throw "$Description failed after $RetryAttempts attempts. Last error: $lastError"
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

function Get-WindowsTarget {
    $architecture = $env:PROCESSOR_ARCHITEW6432
    if (-not $architecture) {
        $architecture = $env:PROCESSOR_ARCHITECTURE
    }
    if (-not $architecture) {
        try {
            $runtimeArchitecture = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture
            if ($null -ne $runtimeArchitecture) {
                $architecture = [string]$runtimeArchitecture
            }
        } catch {
            $architecture = $null
        }
    }

    switch (([string]$architecture).ToUpperInvariant()) {
        "AMD64" { return "x86_64-pc-windows-msvc" }
        "X64" { return "x86_64-pc-windows-msvc" }
        "ARM64" {
            Write-Warning "Windows ARM64 detected; installing the x86_64 build through Windows x64 emulation."
            return "x86_64-pc-windows-msvc"
        }
        default {
            throw "Windows installer supports x86_64 (AMD64) and ARM64 with x64 emulation; detected '$architecture'."
        }
    }
}

$Version = Get-ReleaseVersion
$Target = Get-WindowsTarget

$Archive = "cleanrs-v$Version-$Target.zip"
$BaseUrl = "https://github.com/$Repository/releases/download/v$Version"
$TempDir = Join-Path ([System.IO.Path]::GetTempPath()) ("cleanrs-install-" + [System.IO.Path]::GetRandomFileName())
$ArchivePath = Join-Path $TempDir $Archive
$ManifestPath = Join-Path $TempDir "checksums.txt"

New-Item -ItemType Directory -Path $TempDir -Force | Out-Null
try {
    $checksumsUrl = "$BaseUrl/checksums.txt?version=$Version"
    Invoke-DownloadWithRetry $checksumsUrl $ManifestPath "Downloading installer metadata"
    $manifest = Get-Content -Raw -Path $ManifestPath
    $expected = Get-ExpectedChecksum $manifest $Archive

    Invoke-DownloadWithRetry "$BaseUrl/$Archive?version=$Version" $ArchivePath "Downloading cleanrs $Version for $Target"
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
