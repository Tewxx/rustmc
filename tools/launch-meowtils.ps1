param(
    [Parameter(ValueFromRemainingArguments = $true)][string[]]$ClientArgs = @()
)

$ErrorActionPreference = 'Stop'
try {
    $repo = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
    $build = Join-Path $repo 'tests\forge\build'
    $game = Join-Path $build 'game'
    $client = Join-Path $repo 'target\release\rustmc.exe'
    $runtimeFile = Join-Path $build 'runtime.json'
    $classpathFile = Join-Path $game 'forge_cp.txt'
    foreach ($path in @($client, $runtimeFile, $classpathFile)) {
        if (!(Test-Path -LiteralPath $path -PathType Leaf)) { throw "Required file is missing: $path" }
    }
    $runtime = Get-Content -LiteralPath $runtimeFile -Raw | ConvertFrom-Json
    foreach ($path in @($runtime.stubsJar, $runtime.libjvm)) {
        if (!$path -or !(Test-Path -LiteralPath $path -PathType Leaf)) { throw "Forge runtime file is missing: $path" }
    }
    $mods = Join-Path $game 'mods-forge'
    if (!(Get-ChildItem -LiteralPath $mods -Filter 'Meowtils*.jar' -File)) { throw "Put the Meowtils JAR in $mods" }
    $classpath = (Get-Content -LiteralPath $classpathFile -Raw).Trim()
    if (!$classpath) { throw "Forge classpath is empty: $classpathFile" }
    foreach ($path in $classpath.Split([IO.Path]::PathSeparator, [StringSplitOptions]::RemoveEmptyEntries)) {
        if (!(Test-Path -LiteralPath $path -PathType Leaf)) { throw "Forge library is missing: $path" }
    }
    $tmp = Join-Path $build 'tmp'
    New-Item -ItemType Directory -Force -Path $tmp | Out-Null
    $env:TMP = $tmp
    $env:TEMP = $tmp
    $env:JAVA_HOME = $runtime.javaHome
    $env:RUSTMC_JVM = $runtime.libjvm
    $env:RUSTMC_FORGE_STUBS = $runtime.stubsJar
    $env:RUSTMC_FORGE_CP = $classpath
    $env:RUSTMC_JVM_OPTS = "-Djava.io.tmpdir=$tmp"
    Set-Location -LiteralPath $game
    Write-Host 'Starting rustmc with Meowtils and the mods in mods-forge.'
    $ErrorActionPreference = 'Continue'
    & $client @ClientArgs
    exit $LASTEXITCODE
} catch {
    Write-Host "Meowtils launcher: $($_.Exception.Message)" -ForegroundColor Red
    exit 1
}
