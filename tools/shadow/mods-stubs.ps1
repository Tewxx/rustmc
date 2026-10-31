<#
.SYNOPSIS
Builds the stub jar for the Forge mods in a folder: scans each jar (cached by content hash) and keeps every vanilla
class a mod's Mixins target as real bytecode.
.DESCRIPTION
Static only: modscan never runs mod code. Scans go to tests/forge/build/scan-cache/<sha256>.json; the jar is written to
tests/forge/build/stubs-mods.jar and recorded as runtime.json's stubsJar (run stubs.ps1 with no arguments to go back
to the plain jar). Re-run whenever the mods folder changes.
.EXAMPLE
./tools/shadow/mods-stubs.ps1 -ModsDir tests/forge/build/game/mods-forge
#>
[CmdletBinding()]
param(
    [string]$ModsDir,
    [string]$OutputJar
)

$ErrorActionPreference = 'Stop'
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$build = Join-Path $repo 'tests/forge/build'
if (-not $ModsDir) { $ModsDir = Join-Path $build 'game/mods-forge' }
if (-not $OutputJar) { $OutputJar = Join-Path $build 'stubs-mods.jar' }
$cache = Join-Path $build 'scan-cache'
New-Item -ItemType Directory -Force $cache | Out-Null
$python = Get-Command python -ErrorAction Stop
$javap = Join-Path ((Get-Command javac -ErrorAction SilentlyContinue).Source | Split-Path) 'javap.exe'
if (-not (Test-Path -LiteralPath $javap)) { $javap = 'C:\Program Files\Eclipse Adoptium\jdk-8.0.472.8-hotspot\bin\javap.exe' }

$scans = @()
foreach ($jar in Get-ChildItem -LiteralPath $ModsDir -Filter *.jar | Sort-Object Name) {
    $hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $jar.FullName).Hash.ToLowerInvariant()
    $scan = Join-Path $cache "$hash.json"
    if (-not (Test-Path -LiteralPath $scan)) {
        Write-Host "scanning $($jar.Name)"
        & $python.Source -I (Join-Path $repo 'tools/modscan/modscan.py') $jar.FullName --javap $javap --out $scan
        if ($LASTEXITCODE -ne 0) { throw "modscan failed on $($jar.Name)" }
    }
    $scans += $scan
}
if (-not $scans) { throw "no mod jars in $ModsDir" }
& (Join-Path $PSScriptRoot 'stubs.ps1') -OutputJar $OutputJar -MixinScans $scans
