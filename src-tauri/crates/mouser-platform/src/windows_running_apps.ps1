$items = New-Object System.Collections.Generic.List[object]
$packages = Get-AppxPackage |
  Where-Object { -not [string]::IsNullOrWhiteSpace($_.InstallLocation) } |
  Sort-Object { $_.InstallLocation.Length } -Descending

Get-Process | ForEach-Object {
  $proc = $_
  if ($proc.MainWindowHandle -eq 0) {
    return
  }

  try {
    $path = $proc.Path
  } catch {
    $path = $null
  }

  if ([string]::IsNullOrWhiteSpace($path) -or -not (Test-Path -LiteralPath $path)) {
    return
  }

  $executable = [System.IO.Path]::GetFileName($path)
  if ($executable -ieq "ApplicationFrameHost.exe") {
    return
  }

  $packageFamilyName = $null
  foreach ($pkg in $packages) {
    if ($path.StartsWith($pkg.InstallLocation, [System.StringComparison]::OrdinalIgnoreCase)) {
      $packageFamilyName = $pkg.PackageFamilyName
      break
    }
  }

  $label = $proc.MainWindowTitle
  if ([string]::IsNullOrWhiteSpace($label)) {
    $label = $proc.ProcessName
  }

  $items.Add([pscustomobject]@{
      label = $label
      executable = $executable
      executablePath = $path
      packageFamilyName = $packageFamilyName
    }) | Out-Null
}

$items | Sort-Object executablePath -Unique | ConvertTo-Json -Compress
