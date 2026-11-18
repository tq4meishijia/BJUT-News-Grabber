# Create the portable executable from the Tauri release binary.
# ASCII-only source: Windows PowerShell 5.1 parses BOM-less UTF-8 files as ANSI.
param(
    [string]$Exe = "target/release/cn-app.exe",
    [string]$Out = "target/release/bundle/portable/bjut-news-grabber-0.0.1-portable.exe",
    [string]$Readme = "tools/portable-readme.txt"
)

$ErrorActionPreference = "Stop"

if (-not (Test-Path -LiteralPath $Exe)) {
    $alt = "apps/app/src-tauri/target/release/cn-app.exe"
    if (Test-Path -LiteralPath $alt) {
        $Exe = $alt
    } else {
        throw "Release binary not found: $Exe (also tried $alt)"
    }
}

$outDir = Split-Path -Parent $Out
New-Item -ItemType Directory -Path $outDir -Force | Out-Null
Copy-Item -LiteralPath $Exe -Destination $Out -Force

if (Test-Path -LiteralPath $Readme) {
    Copy-Item -LiteralPath $Readme -Destination (Join-Path $outDir "README.txt") -Force
}

$size = (Get-Item -LiteralPath $Out).Length
Write-Host ("Portable executable created: {0} ({1:N1} MB)" -f $Out, ($size / 1MB))
