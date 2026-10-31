<#
.SYNOPSIS
Builds the local Forge 1.8.9 SRG API stubs and Rust/Java glue on Windows.
.DESCRIPTION
Uses the same ShadowJar and StubJar generators and exact runtime-library list as
the shell tools. All game inputs must already be installed; nothing is downloaded.
The generated jars contain local Minecraft/Forge-derived classes and must not be
redistributed. Runtime paths are written to tests/forge/build/runtime.json.
.EXAMPLE
./tools/shadow/stubs.ps1
.EXAMPLE
./tools/shadow/stubs.ps1 -Jdk8 'C:\Program Files\Eclipse Adoptium\jdk-8.0.472.8-hotspot'
#>
[CmdletBinding()]
param(
    [string]$OutputJar,
    [string]$PrismDir = $env:PRISM,
    [string]$McpDir = $env:MCP,
    [string]$Jdk8 = $env:JDK8,
    [string]$BuildDir = $env:FT_BUILD,
    [string[]]$MixinScans = @(),
    [switch]$ValidateOnly
)

$ErrorActionPreference = 'Stop'
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
if (-not $BuildDir) { $BuildDir = Join-Path $repo 'tests/forge/build' }
$BuildDir = [IO.Path]::GetFullPath($BuildDir)
if (-not $OutputJar) { $OutputJar = Join-Path $BuildDir 'stubs.jar' }
$OutputJar = [IO.Path]::GetFullPath($OutputJar)
if (-not $PrismDir) { $PrismDir = Join-Path $env:APPDATA 'PrismLauncher' }
$librariesDir = Join-Path ([IO.Path]::GetFullPath($PrismDir)) 'libraries'
if (-not $McpDir) {
    $McpDir = Join-Path $repo 'MCP-919'
    if (-not (Test-Path -LiteralPath $McpDir)) { $McpDir = Join-Path $repo '../MCP-919' }
}
$McpDir = [IO.Path]::GetFullPath($McpDir)
if (-not $Jdk8) {
    $candidates = @($env:JAVA_HOME, (Join-Path $repo '.mc-tools/jdk8'))
    foreach ($vendor in @('Eclipse Adoptium', 'Zulu', 'Java')) {
        $vendorDir = Join-Path $env:ProgramFiles $vendor
        if (Test-Path -LiteralPath $vendorDir) {
            $candidates += Get-ChildItem -LiteralPath $vendorDir -Directory | Sort-Object Name -Descending | ForEach-Object { $_.FullName }
        }
    }
    $Jdk8 = $candidates | Where-Object { $_ -and (Test-Path -LiteralPath (Join-Path $_ 'jre/lib/rt.jar')) } | Select-Object -First 1
}
if (-not $Jdk8) { throw 'JDK 8 not found. Supply -Jdk8 or set JDK8 to its installation directory.' }
$Jdk8 = [IO.Path]::GetFullPath($Jdk8)
$java = Join-Path $Jdk8 'bin/java.exe'
$javac = Join-Path $Jdk8 'bin/javac.exe'
$jarTool = Join-Path $Jdk8 'bin/jar.exe'
$libjvm = Join-Path $Jdk8 'jre/bin/server/jvm.dll'
$rtJar = Join-Path $Jdk8 'jre/lib/rt.jar'
$forgeVersion = '1.8.9-11.15.1.2318-1.8.9'
$forgeJar = Join-Path $librariesDir "net/minecraftforge/forge/$forgeVersion/forge-$forgeVersion-universal.jar"
$asmJar = Join-Path $librariesDir 'org/ow2/asm/asm-all/5.0.3/asm-all-5.0.3.jar'
$lzmaJar = Join-Path $librariesDir 'lzma/lzma/0.0.1/lzma-0.0.1.jar'
$launchJar = Join-Path $librariesDir 'net/minecraft/launchwrapper/1.12/launchwrapper-1.12.jar'
$vanillaJar = Join-Path $McpDir 'jars/versions/1.8.9/1.8.9.jar'
if (-not (Test-Path -LiteralPath $vanillaJar)) {
    $vanillaJar = Join-Path $librariesDir 'com/mojang/minecraft/1.8.9/minecraft-1.8.9-client.jar'
}

# Keep one version of each library, in the same order as the existing harness.
# A recursive *.jar classpath would mix incompatible modern Minecraft libraries.
$envSource = Get-Content -LiteralPath (Join-Path $repo 'tools/forgetest/env.sh') -Raw
$runtimeMatch = [regex]::Match($envSource, '(?s)RUNTIME_LIBS="(.*?)"')
if (-not $runtimeMatch.Success) { throw 'Could not read RUNTIME_LIBS from tools/forgetest/env.sh.' }
$runtimeLibraries = @($runtimeMatch.Groups[1].Value -split '\r?\n' |
    ForEach-Object { $_.Trim() } | Where-Object { $_ } |
    ForEach-Object { Join-Path $librariesDir $_ })
$toolsSources = @(
    (Join-Path $repo 'tools/forgespec/ForgeSpec.java'),
    (Join-Path $repo 'tools/forgespec/EventScan.java'),
    (Join-Path $PSScriptRoot 'ShadowJar.java'),
    (Join-Path $PSScriptRoot 'StubJar.java')
)
$mappings = @('joined.srg', 'methods.csv', 'fields.csv') | ForEach-Object { Join-Path $McpDir "conf/$_" }
$required = @($java, $javac, $jarTool, $libjvm, $rtJar, $forgeJar, $asmJar, $lzmaJar, $launchJar, $vanillaJar) + $mappings + $toolsSources + $runtimeLibraries
$missing = @($required | Where-Object { -not (Test-Path -LiteralPath $_ -PathType Leaf) })
if ($missing.Count) { throw "Required local inputs are missing:`n$($missing -join "`n")" }
if ($OutputJar -eq $forgeJar -or $OutputJar -eq $vanillaJar -or $OutputJar -eq (Join-Path $BuildDir 'shadow-forge.jar')) {
    throw 'OutputJar must not replace an input jar or the cached shadow jar.'
}
if ($ValidateOnly) {
    [pscustomobject]@{ JavaHome = $Jdk8; VanillaJar = $vanillaJar; ForgeJar = $forgeJar; RuntimeLibraries = $runtimeLibraries.Count; OutputJar = $OutputJar }
    return
}

function Invoke-JavaTool {
    param([string]$Executable, [string[]]$Arguments)
    # Windows PowerShell 5.1 turns any native stderr line (javac notes) into a terminating error under Stop.
    $ErrorActionPreference = 'Continue'
    & $Executable @Arguments 2>&1 | ForEach-Object { "$_" }
    if ($LASTEXITCODE -ne 0) { throw "$Executable failed with exit code $LASTEXITCODE." }
}

New-Item -ItemType Directory -Path $BuildDir -Force | Out-Null
New-Item -ItemType Directory -Path ([IO.Path]::GetDirectoryName($OutputJar)) -Force | Out-Null
# Unique staging directories avoid deleting another invocation's compiled classes.
$runDir = Join-Path $BuildDir ('stub-build-' + [guid]::NewGuid().ToString('N'))
$toolClasses = Join-Path $runDir 'tools'
$glueClasses = Join-Path $runDir 'glue'
New-Item -ItemType Directory -Path $toolClasses, $glueClasses -Force | Out-Null
$toolCp = @($asmJar, $lzmaJar, $forgeJar) -join [IO.Path]::PathSeparator
Invoke-JavaTool $javac (@('-nowarn', '-encoding', 'UTF-8', '-cp', $toolCp, '-d', $toolClasses) + $toolsSources)
$generatorCp = @($toolClasses, $asmJar, $lzmaJar, $forgeJar) -join [IO.Path]::PathSeparator
$shadowJar = Join-Path $BuildDir 'shadow-forge.jar'
$shadowFingerprint = Join-Path $BuildDir 'shadow-forge.inputs.json'
$shadowInputs = @($vanillaJar, $forgeJar, $asmJar, $lzmaJar) + $mappings + $toolsSources[0..2]
$fingerprint = @($shadowInputs | ForEach-Object { [ordered]@{ path = $_; sha256 = (Get-FileHash -LiteralPath $_ -Algorithm SHA256).Hash } }) | ConvertTo-Json -Depth 3
$cachedFingerprint = if (Test-Path -LiteralPath $shadowFingerprint) { Get-Content -LiteralPath $shadowFingerprint -Raw } else { '' }
if (-not (Test-Path -LiteralPath $shadowJar) -or $cachedFingerprint.Trim() -ne $fingerprint.Trim()) {
    $newShadow = Join-Path $runDir 'shadow-forge.jar'
    Invoke-JavaTool $java (@('-Dshadow.forge=true', '-cp', $generatorCp, 'ShadowJar', $vanillaJar, $forgeJar) + $mappings + @($newShadow))
    Move-Item -LiteralPath $newShadow -Destination $shadowJar -Force
    Set-Content -LiteralPath $shadowFingerprint -Value $fingerprint -Encoding UTF8
}

$keep = @('fml/repackage/', '^(?!net/minecraft/|net/minecraftforge/|org/lwjgl/)')
$keep += Get-Content -LiteralPath (Join-Path $PSScriptRoot 'keep-real.txt') | Where-Object { $_.Trim() -and -not $_.StartsWith('#') }
if ($env:STUB_KEEP) { $keep += $env:STUB_KEEP }
if (-not $MixinScans.Count -and $env:STUB_MIXIN_SCANS) {
    throw 'On Windows, pass scans as -MixinScans @("a.json", "b.json") instead of the whitespace-separated STUB_MIXIN_SCANS variable.'
}
if ($MixinScans.Count) {
    $python = Get-Command python -ErrorAction Stop
    # Its "# SKIPPED" notes go to stderr, which Windows PowerShell would turn into a terminating error.
    $previous = $ErrorActionPreference; $ErrorActionPreference = 'Continue'
    $output = & $python.Source -I (Join-Path $PSScriptRoot 'mixin-keep.py') @MixinScans 2>&1
    $ErrorActionPreference = $previous
    if ($LASTEXITCODE -ne 0) { throw "mixin-keep.py failed with exit code $LASTEXITCODE." }
    $output | Where-Object { $_ -is [System.Management.Automation.ErrorRecord] } | ForEach-Object { Write-Warning $_.ToString() }
    $patterns = @($output | Where-Object { $_ -is [string] })
    $keep += $patterns
}
$stagedJar = Join-Path $runDir 'stubs.jar'
$lwjgl = @(
    (Join-Path $librariesDir 'org/lwjgl/lwjgl/lwjgl/2.9.4-nightly-20150209/lwjgl-2.9.4-nightly-20150209.jar'),
    (Join-Path $librariesDir 'org/lwjgl/lwjgl/lwjgl_util/2.9.4-nightly-20150209/lwjgl_util-2.9.4-nightly-20150209.jar')
)
Invoke-JavaTool $java (@('-cp', $generatorCp, 'StubJar', $stagedJar, ($keep -join '|'), (Join-Path $PSScriptRoot 'stub-methods.txt'), $rtJar) + $runtimeLibraries + @('--', $shadowJar, $forgeJar) + $lwjgl)
$glueSources = @(Get-ChildItem -LiteralPath (Join-Path $repo 'crates/mc-forge/java/rustmc') -Filter '*.java' -File -Recurse | ForEach-Object { $_.FullName })
$glueCp = (@($stagedJar) + $runtimeLibraries) -join [IO.Path]::PathSeparator
Invoke-JavaTool $javac (@('-nowarn', '-encoding', 'UTF-8', '-cp', $glueCp, '-d', $glueClasses) + $glueSources)
Invoke-JavaTool $jarTool @('uf', $stagedJar, '-C', $glueClasses, 'rustmc')
Move-Item -LiteralPath $stagedJar -Destination $OutputJar -Force

$runtimeCp = $runtimeLibraries -join [IO.Path]::PathSeparator
$manifest = [ordered]@{
    schema = 1
    stubsJar = $OutputJar
    shadowJar = $shadowJar
    javaHome = $Jdk8
    libjvm = $libjvm
    classpath = $runtimeCp
    libraries = $runtimeLibraries
}
$manifestPath = Join-Path $BuildDir 'runtime.json'
# No BOM: Windows PowerShell's -Encoding UTF8 writes one, which serde_json rejects.
[IO.File]::WriteAllText($manifestPath, ($manifest | ConvertTo-Json -Depth 4), (New-Object Text.UTF8Encoding $false))
$env:RUSTMC_STUBS_JAR = $OutputJar
$env:RUSTMC_FORGE_STUBS = $OutputJar
$env:RUSTMC_FORGE_CP = $runtimeCp
$env:RUSTMC_JVM = $libjvm
Write-Host "Built $OutputJar"
Write-Host "Runtime paths: $manifestPath"
Write-Host 'This PowerShell session now has RUSTMC_STUBS_JAR, RUSTMC_FORGE_STUBS, RUSTMC_FORGE_CP and RUSTMC_JVM set.'
