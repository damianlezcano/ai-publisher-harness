$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$manifest = Get-Content (Join-Path $root 'config/components.json') -Raw | ConvertFrom-Json
$triple = 'x86_64-pc-windows-msvc'
$sidecars = Join-Path $root 'sidecars'
New-Item -ItemType Directory -Force -Path $sidecars | Out-Null

foreach ($component in $manifest.components | Where-Object { $_.platform -eq 'windows-x86_64' }) {
  $download = Join-Path $env:TEMP ("educai-" + $component.name + ".download")
  Invoke-WebRequest -Uri $component.source -OutFile $download
  $actual = (Get-FileHash -Algorithm SHA256 $download).Hash.ToLowerInvariant()
  if ($actual -ne $component.sha256) { Remove-Item -Force $download; throw "checksum mismatch for $($component.name)" }
  $destination = Join-Path $sidecars ("$($component.bundleName)-$triple.exe")
  if ($component.format -eq 'zip') {
    $extract = Join-Path $env:TEMP ("educai-" + [guid]::NewGuid())
    Expand-Archive -Path $download -DestinationPath $extract -Force
    $candidate = Get-ChildItem -Path $extract -Recurse -Filter "$($component.name).exe" | Select-Object -First 1
    if ($null -eq $candidate) { throw "missing $($component.name).exe in archive" }
    Copy-Item $candidate.FullName $destination -Force
    Remove-Item -Recurse -Force $extract
  } else { Copy-Item $download $destination -Force }
  Remove-Item -Force $download
}

pnpm --dir (Join-Path $root 'app') install --frozen-lockfile
Push-Location (Join-Path $root 'app/src-tauri')
cargo tauri build --bundles nsis
Pop-Location
$artifact = Get-ChildItem (Join-Path $root 'app/src-tauri/target/release/bundle/nsis') -Filter '*.exe' | Select-Object -First 1
if ($null -eq $artifact) { throw 'no NSIS artifact produced' }
Get-FileHash -Algorithm SHA256 $artifact.FullName
