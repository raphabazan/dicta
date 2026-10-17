param([Parameter(Mandatory=$true)][string]$ProfilePath, [Parameter(Mandatory=$true)][string]$AudioPath)
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$exe = Join-Path $repo 'src-tauri\target\debug\dicta.exe'
if (-not (Test-Path -LiteralPath $exe)) { throw 'Compile o binário de desenvolvimento antes de iniciar o teste.' }
$env:DICTA_DATA_DIR = [IO.Path]::GetFullPath($ProfilePath)
$env:DICTA_TEST_AUDIO = [IO.Path]::GetFullPath($AudioPath)
$env:DICTA_SHOW_MAIN = '1'
$env:DICTA_TEST_HOTKEYS = '1'
$process = Start-Process -FilePath $exe -WorkingDirectory $repo -WindowStyle Hidden -PassThru
Write-Output "Isolated test process: $($process.Id). Profile: $env:DICTA_DATA_DIR"
