$rackdirector = Start-Job -Name 'rack-director' -ScriptBlock {
  Write-Host "Starting Rack Director"
  $env:LOG = 'debug'
  $env:RACK_DIRECTOR_UI_PATH = 'rack-director-ui\dist'
  $ErrorActionPreference = 'Continue'
  Set-Location -Path $using:PWD
  & 'cargo' @(
    'run', '--bin', 'rack-director', '--',
    '--db-path', '.',
    '--storage-path', './.local-storage/data',
    '--tftp-path', './.local-storage/tftp',
    '--agent-images-path', './.local-storage/agent-image',
    '--dhcp-address', '127.0.0.1:1067',
    '--dhcp-server-identifier', '127.0.0.1',
    '--tftp-address', '127.0.0.1:1069',
    '--tftp-public-address', '127.0.0.1',
    '--http-public-url', 'http://127.0.0.1') 2>&1 | ForEach-Object { "$_" }
}

$vite = Start-Job -Name 'npx vite build' -ScriptBlock {
  Set-Location -Path "$using:PWD/rack-director-ui"
  & (Get-Command 'npx').Source 'vite' 'build' '--watch' 2>&1 | ForEach-Object { "$_" }
}

try {
  Start-Sleep 2

  Write-Host "Started rack-director and vite"

  while (Get-Job) {
    Get-Job | Receive-Job
    if ($vite.Finished -or $rackdirector.Finished) {
      if (! $rackdirector.Finished) {
        $rackdirector.StopJob()
      }
      if (! $vite.Finished) {
        $vite.StopJob()
      }
      
      if ($rackdirector.Finished -and !$rackdirector.HasMoreData) {
        Remove-Job -Job $rackdirector
      }

      if ($vite.Finished -and !$vite.HasMoreData) {
        Remove-Job -Job $vite
      }
    }

    Start-Sleep 1
  }
} finally {
  Write-Host "Stopping background jobs"
  Get-Job | Stop-Job -PassThru | Receive-Job -Force -AutoRemoveJob -Wait
}
