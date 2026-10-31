<# Builds this controlled fixture only; generate the local stubs first. #>
[CmdletBinding()]
param([string]$RuntimeManifest, [string]$MixinJar, [string]$AsmVersion = '9.5')
$ErrorActionPreference = 'Stop'
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../../../../..'))
if (-not $RuntimeManifest) { $RuntimeManifest = Join-Path $repo 'tests/forge/build/runtime.json' }
$runtime = Get-Content -LiteralPath $RuntimeManifest -Raw | ConvertFrom-Json
$prismLibraries = if ($env:PRISM) { Join-Path $env:PRISM 'libraries' } else { Join-Path $env:APPDATA 'PrismLauncher/libraries' }
if (-not $MixinJar) {
    $MixinJar = Join-Path $prismLibraries 'org/spongepowered/mixin/0.8.5/mixin-0.8.5.jar'
}
if (-not (Test-Path -LiteralPath $MixinJar -PathType Leaf)) { throw 'Supply -MixinJar pointing to a local Sponge Mixin jar.' }
$MixinJar = [IO.Path]::GetFullPath($MixinJar)
# The generator deliberately uses ASM 5's old RemappingClassAdapter. Modern
# Mixin needs ASM 9 at runtime; do not let an earlier asm-all shadow these modules.
$asmLibraries = @('asm', 'asm-tree', 'asm-commons', 'asm-util', 'asm-analysis') |
    ForEach-Object { Join-Path $prismLibraries "org/ow2/asm/$_/$AsmVersion/$_-$AsmVersion.jar" }
$missingAsm = @($asmLibraries | Where-Object { -not (Test-Path -LiteralPath $_ -PathType Leaf) })
if ($missingAsm.Count) { throw "A complete local ASM $AsmVersion module set is required:`n$($missingAsm -join "`n")" }
$baseLibraries = @($runtime.libraries | Where-Object { $_ -notmatch '[\\/]org[\\/]ow2[\\/]asm[\\/]' })
# Mixin 0.8 needs Guava 21 (MoreFiles/RecursiveDeleteOption); 1.8.9 ships 17.
$guava21 = Join-Path $prismLibraries 'com/google/guava/guava/21.0/guava-21.0.jar'
if (-not (Test-Path -LiteralPath $guava21 -PathType Leaf)) { throw "Mixin 0.8 requires $guava21." }
$baseLibraries = @($baseLibraries | ForEach-Object { if ($_ -match '[\\/]guava-17\.0\.jar$') { $guava21 } else { $_ } })
$fixtureLibraries = @($asmLibraries) + $baseLibraries + @($MixinJar)
$fixtureCp = $fixtureLibraries -join [IO.Path]::PathSeparator
$build = [IO.Path]::GetDirectoryName([IO.Path]::GetFullPath($RuntimeManifest))
$stage = Join-Path $build ('liveprobe-' + [guid]::NewGuid().ToString('N'))
$classes = Join-Path $stage 'classes'
New-Item -ItemType Directory -Path $classes -Force | Out-Null
$sources = @(Get-ChildItem -LiteralPath (Join-Path $PSScriptRoot 'ft') -Recurse -Filter '*.java' -File | ForEach-Object { $_.FullName })
$cp = @($runtime.stubsJar, $fixtureCp) -join [IO.Path]::PathSeparator
# Windows PowerShell 5.1 turns javac's stderr notes into terminating errors under Stop.
$ErrorActionPreference = 'Continue'
& (Join-Path $runtime.javaHome 'bin/javac.exe') -nowarn -proc:none -encoding UTF-8 -cp $cp -d $classes @sources 2>&1 | ForEach-Object { "$_" }
$ErrorActionPreference = 'Stop'
if ($LASTEXITCODE -ne 0) { throw "Probe javac failed with exit code $LASTEXITCODE." }
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'mixins.rustmc-liveprobe.json') -Destination $classes
$stagedJar = Join-Path $stage 'liveprobe.jar'
& (Join-Path $runtime.javaHome 'bin/jar.exe') cfm $stagedJar (Join-Path $PSScriptRoot 'manifest.txt') -C $classes .
if ($LASTEXITCODE -ne 0) { throw "Probe jar packaging failed with exit code $LASTEXITCODE." }
$probeJar = Join-Path $build 'liveprobe.jar'
Move-Item -LiteralPath $stagedJar -Destination $probeJar -Force
$env:RUSTMC_LIVE_PROBE_JAR = $probeJar
$env:RUSTMC_FORGE_CP = $fixtureCp
# No BOM: Windows PowerShell's -Encoding UTF8 writes one, which serde_json rejects.
$manifestJson = [ordered]@{ probeJar = $probeJar; mixinJar = $MixinJar; asmVersion = $AsmVersion; asmLibraries = $asmLibraries; classpath = $env:RUSTMC_FORGE_CP } | ConvertTo-Json
[IO.File]::WriteAllText((Join-Path $build 'liveprobe-runtime.json'), $manifestJson, (New-Object Text.UTF8Encoding $false))
Write-Host "Built controlled probe: $probeJar"
