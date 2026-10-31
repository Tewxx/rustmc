param(
    [ValidateRange(2, 600)][int]$Seconds = 20,
    [ValidateRange(1, 10)][int]$Repeats = 3,
    [ValidateSet('quiet', 'stream4', 'stream16', 'crowd1000', 'edits200')][string[]]$Scenarios = @('quiet', 'stream4', 'crowd1000', 'edits200'),
    [string]$Client,
    [string]$Reference,
    [ValidateRange(1024, 65535)][int]$Port = 25599
)

$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
if (!$Client) { $Client = Join-Path $repo 'target\release\rustmc.exe' }
$clients = @([pscustomobject]@{ name = 'current'; path = (Resolve-Path -LiteralPath $Client).Path })
if ($Reference) { $clients = @([pscustomobject]@{ name = 'reference'; path = (Resolve-Path -LiteralPath $Reference).Path }) + $clients }
$serverPath = (Resolve-Path -LiteralPath (Join-Path $repo 'target\release\examples\mock_server.exe')).Path
$game = Join-Path $repo 'tests\forge\build\game'
$tmp = Join-Path $repo 'tests\forge\build\tmp'
$output = Join-Path $tmp ('bench-' + (Get-Date -Format 'yyyyMMdd-HHmmss') + '-' + [guid]::NewGuid().ToString('N').Substring(0, 8))
New-Item -ItemType Directory -Force -Path $game, $tmp, $output | Out-Null
$savedEnv = @{}
$names = @('TMP', 'TEMP', 'RUSTMC_FORGE_STUBS', 'RUSTMC_RENDER_TIMING') + @(Get-ChildItem Env:MOCK_* | ForEach-Object Name) + @('MOCK_STREAM', 'MOCK_CROWD', 'MOCK_EDITS')
foreach ($name in ($names | Select-Object -Unique)) { $savedEnv[$name] = [Environment]::GetEnvironmentVariable($name, 'Process') }
$results = [System.Collections.Generic.List[object]]::new()
try {
    $env:TMP = $tmp; $env:TEMP = $tmp
    Remove-Item Env:RUSTMC_FORGE_STUBS -ErrorAction SilentlyContinue
    $env:RUSTMC_RENDER_TIMING = '1'
    $metadata = [ordered]@{ seconds = $Seconds; repeats = $Repeats; scenarios = $Scenarios; player_name = 'Player'; binaries = @(); settings = @{} }
    foreach ($clientBuild in $clients) {
        $metadata.binaries += @{ name = $clientBuild.name; path = $clientBuild.path; sha256 = (Get-FileHash -LiteralPath $clientBuild.path).Hash }
    }
    foreach ($name in @('WGPU_BACKEND', 'RUSTMC_PRESENT_MODE', 'RUSTMC_FRAME_LATENCY', 'RUSTMC_NO_UPLOAD_POOL', 'RUSTMC_NO_GUI_INDEXING', 'RUSTMC_NO_TRANSFORM_CACHE', 'RUSTMC_NO_ITEM_INSTANCING', 'RUSTMC_NO_SKY_INDEXING', 'RUSTMC_NO_ENTITY_CULLING')) {
        $metadata.settings[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
    }
    $metadata | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $output 'metadata.json')
    foreach ($scenario in $Scenarios) {
        Get-ChildItem Env:MOCK_* | Remove-Item
        switch ($scenario) {
            'stream4' { $env:MOCK_STREAM = '4' }
            'stream16' { $env:MOCK_STREAM = '16' }
            'crowd1000' { $env:MOCK_CROWD = '1000' }
            'edits200' { $env:MOCK_EDITS = '200' }
        }
        $probe = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, $Port)
        try { $probe.Start() } finally { $probe.Stop() }
        $server = Start-Process -FilePath $serverPath -ArgumentList @([string]$Port) -WorkingDirectory $game -WindowStyle Hidden -PassThru `
            -RedirectStandardOutput (Join-Path $output ($scenario + '-server.log')) -RedirectStandardError (Join-Path $output ($scenario + '-server.err'))
        try {
            for ($repeat = 0; $repeat -lt $Repeats; $repeat++) {
                $ordered = @($clients)
                if ($repeat % 2) { [Array]::Reverse($ordered) }
                foreach ($clientBuild in $ordered) {
                    if ($server.HasExited) { throw 'Mock server exited; see server logs.' }
                    $tag = $scenario + '-' + $clientBuild.name + '-' + ($repeat + 1)
                    $log = Join-Path $output ($tag + '.log')
                    Push-Location $game
                    try {
                        & $clientBuild.path --connect "127.0.0.1:$Port" --name Player --bench $Seconds *> $log
                        if ($LASTEXITCODE -ne 0) { throw "Client failed: $log" }
                        $summary = Get-Content -LiteralPath $log | Where-Object { $_ -match '^bench: .*avg ([0-9.]+) fps, 1% low ([0-9.]+) fps, 0.1% low ([0-9.]+) fps' } | Select-Object -First 1
                        if (!$summary) { throw "Benchmark did not complete: $log" }
                        $null = $summary -match 'avg ([0-9.]+) fps, 1% low ([0-9.]+) fps, 0.1% low ([0-9.]+) fps'
                        $results.Add([pscustomobject]@{ scenario = $scenario; build = $clientBuild.name; repeat = $repeat + 1; avg_fps = $Matches[1]; low1_fps = $Matches[2]; low01_fps = $Matches[3] })
                        Copy-Item -LiteralPath bench.csv -Destination (Join-Path $output ($tag + '.csv'))
                        Write-Output "$tag $summary"
                    } finally { Pop-Location }
                }
            }
        } finally { Stop-Process -Id $server.Id -ErrorAction SilentlyContinue }
    }
    $results | Export-Csv -NoTypeInformation -LiteralPath (Join-Path $output 'summary.csv')
    Write-Output "Saved $output"
} finally {
    foreach ($name in $savedEnv.Keys) { [Environment]::SetEnvironmentVariable($name, $savedEnv[$name], 'Process') }
}
