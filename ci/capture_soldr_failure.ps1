# Windows ARM failure evidence for the pinned Soldr v0.7.51/zccache 1.11.8.
# `soldr logs paths` was added later; enumerate only its known log locations.
param([string]$OutputDir = "soldr-failure-diagnostics")

$ErrorActionPreference = "Continue"
$ProbeTimeoutMs = 15000
$MaxLogFiles = 24
$MaxLogBytes = 65536
$MaxPrivateDirs = 8
$captured = 0
New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null
$inventory = Join-Path $OutputDir "logs-paths.txt"
"Soldr v0.7.51 log paths and bounded captures:" | Out-File -FilePath $inventory -Encoding utf8

function Save-LogTail {
    param([string]$Source)
    if ($script:captured -ge $MaxLogFiles -or -not (Test-Path -LiteralPath $Source -PathType Leaf)) {
        return
    }
    try {
        $stream = [System.IO.File]::Open($Source, 'Open', 'Read', 'ReadWrite')
        try {
            $bytesToRead = [int][Math]::Min($stream.Length, $MaxLogBytes)
            $null = $stream.Seek(-$bytesToRead, 'End')
            $bytes = New-Object byte[] $bytesToRead
            $read = $stream.Read($bytes, 0, $bytesToRead)
            $script:captured++
            $target = Join-Path $OutputDir ("log-{0:D2}-{1}" -f $script:captured, [IO.Path]::GetFileName($Source))
            if ($read -gt 0) {
                $capturedBytes = New-Object byte[] $read
                [Array]::Copy($bytes, $capturedBytes, $read)
                [System.IO.File]::WriteAllBytes($target, $capturedBytes)
            } else {
                [System.IO.File]::WriteAllBytes($target, [byte[]]@())
            }
            "captured=$target source=$Source bytes=$read original_bytes=$($stream.Length)" |
                Add-Content -Path $inventory
        } finally {
            $stream.Dispose()
        }
    } catch {
        "unreadable=$Source reason=$_" | Add-Content -Path $inventory
    }
}

function Save-KnownLogDirectory {
    param([string]$Directory)
    if (-not $Directory -or -not (Test-Path -LiteralPath $Directory -PathType Container)) {
        return
    }
    "checked=$Directory" | Add-Content -Path $inventory
    Get-ChildItem -LiteralPath $Directory -File -ErrorAction SilentlyContinue |
        Where-Object { $_.Name -match '^(daemon.*\.log(\..*)?|last-session\.(log|jsonl)|lifecycle\.jsonl|embedded-.*\.warn\.log.*)$' } |
        Sort-Object LastWriteTime -Descending |
        Select-Object -First $MaxLogFiles |
        ForEach-Object { Save-LogTail $_.FullName }
}

$soldr = $env:SOLDR_BINARY
if (-not $soldr -or -not (Test-Path -LiteralPath $soldr)) {
    $resolved = Get-Command soldr -ErrorAction SilentlyContinue
    if ($resolved) { $soldr = $resolved.Source }
}
if ($soldr -and (Test-Path -LiteralPath $soldr)) {
    foreach ($probe in @('doctor', 'status')) {
        $stdout = Join-Path $env:RUNNER_TEMP "soldr-$probe-stdout.txt"
        $stderr = Join-Path $env:RUNNER_TEMP "soldr-$probe-stderr.txt"
        try {
            $process = Start-Process -FilePath $soldr -ArgumentList $probe -PassThru -NoNewWindow `
                -RedirectStandardOutput $stdout -RedirectStandardError $stderr
            if (-not $process.WaitForExit($ProbeTimeoutMs)) {
                $process.Kill($true)
                $null = $process.WaitForExit(2000)
                "probe=$probe timed_out_ms=$ProbeTimeoutMs" | Add-Content -Path $inventory
            } else {
                "probe=$probe exit_code=$($process.ExitCode)" | Add-Content -Path $inventory
            }
        } catch {
            "probe=$probe error=$_" | Add-Content -Path $inventory
        }
        Save-LogTail $stdout
        Save-LogTail $stderr
    }
} else {
    "soldr binary unavailable; SOLDR_BINARY=$($env:SOLDR_BINARY)" | Add-Content -Path $inventory
}

$setupRoot = Join-Path $env:RUNNER_TEMP 'setup-soldr-soldr'
$zccacheRoot = Join-Path $setupRoot 'cache\zccache'
Save-KnownLogDirectory (Join-Path $zccacheRoot 'logs')
Save-KnownLogDirectory (Join-Path $setupRoot 'cache\soldr-daemon')
Save-LogTail (Join-Path $setupRoot 'daemon-spawn.log')

# The pinned Soldr puts private zccache daemons under private/<name>.
$privateRoot = Join-Path $zccacheRoot 'private'
if (Test-Path -LiteralPath $privateRoot -PathType Container) {
    Get-ChildItem -LiteralPath $privateRoot -Directory -ErrorAction SilentlyContinue |
        Select-Object -First $MaxPrivateDirs |
        ForEach-Object { Save-KnownLogDirectory (Join-Path $_.FullName 'logs') }
}

# The runner's Soldr home may hold a separate daemon lifecycle log.
$homeRoot = Join-Path $env:USERPROFILE '.soldr'
Save-KnownLogDirectory (Join-Path $homeRoot 'cache\soldr-daemon')
Save-LogTail (Join-Path $homeRoot 'daemon-spawn.log')
"captured_files=$captured max_files=$MaxLogFiles max_bytes_per_file=$MaxLogBytes" |
    Add-Content -Path $inventory
