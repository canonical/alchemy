Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$repo = if ($env:ALCHEMY_INSTALL_REPO) { $env:ALCHEMY_INSTALL_REPO } else { "canonical/alchemy" }
$version = if ($env:ALCHEMY_VERSION) { $env:ALCHEMY_VERSION } else { "latest" }
$installDir = if ($env:ALCHEMY_INSTALL_DIR) { $env:ALCHEMY_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA "Programs\alchemy" }

$arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
switch ($arch) {
    "X64" { $target = "x86_64-pc-windows-msvc" }
    "Arm64" { $target = "aarch64-pc-windows-msvc" }
    default { throw "Unsupported architecture: $arch. Supported: X64, Arm64." }
}

if ($version -eq "latest") {
    $latestResponse = Invoke-WebRequest -Uri "https://github.com/$repo/releases/latest" -MaximumRedirection 10
    $latestUrl = $latestResponse.BaseResponse.ResponseUri.AbsoluteUri
    if (-not $latestUrl -or $latestUrl -notmatch "/releases/tag/([^/?#]+)") {
        throw "Failed to resolve latest release tag for $repo."
    }
    $tag = $matches[1]
} else {
    $tag = $version
}

$asset = "alchemy-$target.zip"
$url = "https://github.com/$repo/releases/download/$tag/$asset"

$tmpDir = Join-Path ([System.IO.Path]::GetTempPath()) ("alchemy-install-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $tmpDir | Out-Null

try {
    $archivePath = Join-Path $tmpDir $asset
    Write-Host "Downloading $url"
    Invoke-WebRequest -Uri $url -OutFile $archivePath

    $extractDir = Join-Path $tmpDir "extract"
    Expand-Archive -Path $archivePath -DestinationPath $extractDir -Force

    $binary = Get-ChildItem -Path $extractDir -Recurse -Filter "alchemy.exe" | Select-Object -First 1
    if (-not $binary) {
        throw "Downloaded archive does not contain alchemy.exe."
    }

    New-Item -ItemType Directory -Path $installDir -Force | Out-Null
    $dest = Join-Path $installDir "alchemy.exe"
    Copy-Item -Path $binary.FullName -Destination $dest -Force
    Write-Host "Installed to $dest"

    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if (-not $userPath) {
        $userPath = ""
    }
    $segments = $userPath -split ";" | Where-Object { $_ -ne "" }
    if ($segments -notcontains $installDir) {
        $newUserPath = if ($userPath.Trim()) { "$userPath;$installDir" } else { $installDir }
        [Environment]::SetEnvironmentVariable("Path", $newUserPath, "User")
        Write-Host "Added $installDir to your user PATH. Restart your terminal to use 'alchemy'."
    } else {
        Write-Host "Run: alchemy --help"
    }
}
finally {
    if (Test-Path $tmpDir) {
        Remove-Item -Path $tmpDir -Recurse -Force
    }
}
