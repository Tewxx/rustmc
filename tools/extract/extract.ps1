# Compiles the golden-data extractor against the MCP 1.8.9 classes and writes dumps to <repo>/golden.
# Usage: powershell -File tools/extract/extract.ps1 [-Mcp ..\MCP-919] [dump names...]
# Needs a JDK 8 (what 1.8.9 ships with): $env:JAVA8_HOME, or one of the usual install paths.
[CmdletBinding(PositionalBinding = $false)]
param(
    [string]$Mcp = "",
    [Parameter(ValueFromRemainingArguments = $true)][string[]]$Dumps = @()
)
$ErrorActionPreference = "Stop"
# Windows PowerShell 5.1 doesn't set $PSScriptRoot while evaluating param defaults.
if (-not $Mcp) { $Mcp = Join-Path $PSScriptRoot "..\..\..\MCP-919" }

$repo = Resolve-Path (Join-Path $PSScriptRoot "..\..")
$Mcp = Resolve-Path $Mcp
$classes = Join-Path $Mcp "bin\minecraft"
if (-not (Test-Path (Join-Path $classes "net\minecraft\block\Block.class"))) {
    throw "No compiled MCP classes in $classes (run MCP's recompile first)"
}

$jdk = $null
$candidates = @($env:JAVA8_HOME) + (Get-ChildItem "C:\Program Files\Eclipse Adoptium\jdk-8*", "C:\Program Files\Java\jdk-1.8*", "C:\Program Files\Java\jdk1.8*", "C:\Program Files\Zulu\zulu-8*" -ErrorAction SilentlyContinue | ForEach-Object FullName)
foreach ($c in $candidates) {
    if ($c -and (Test-Path (Join-Path $c "bin\javac.exe"))) { $jdk = $c; break }
}
if (-not $jdk) { throw "JDK 8 not found; set JAVA8_HOME" }

$libs = Get-ChildItem (Join-Path $Mcp "jars\libraries") -Recurse -Filter *.jar | Where-Object { $_.Name -notmatch "natives" } | ForEach-Object FullName
# The vanilla jar supplies resources Bootstrap reads (lang files); MCP classes come first so they win.
$jar = Join-Path $Mcp "jars\versions\1.8.9\1.8.9.jar"
$cp = (@($classes, $jar) + $libs) -join ";"

$out = Join-Path $PSScriptRoot "out"
$out = [System.IO.Path]::GetFullPath($out)
$expectedOut = [System.IO.Path]::GetFullPath((Join-Path $repo "tools\extract\out"))
if ($out -ne $expectedOut) { throw "Refusing to clean unexpected extractor output path: $out" }
if (Test-Path -LiteralPath $out) { Remove-Item -LiteralPath $out -Recurse -Force }
New-Item -ItemType Directory -Force $out | Out-Null
$sources = Get-ChildItem (Join-Path $PSScriptRoot "src") -Recurse -Filter *.java | ForEach-Object FullName

& (Join-Path $jdk "bin\javac.exe") -nowarn -encoding UTF-8 -cp $cp -d $out @sources
if ($LASTEXITCODE -ne 0) { throw "javac failed" }

& (Join-Path $jdk "bin\java.exe") -cp "$out;$cp" rustmc.extract.Main (Join-Path $repo "golden") @Dumps
if ($LASTEXITCODE -ne 0) { throw "extractor failed" }
