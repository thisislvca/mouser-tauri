$items = New-Object System.Collections.Generic.List[object]

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

  $label = $proc.MainWindowTitle
  if ([string]::IsNullOrWhiteSpace($label)) {
    $label = $proc.ProcessName
  }

  $items.Add([pscustomobject]@{
      label = $label
      executable = [System.IO.Path]::GetFileName($path)
      executablePath = $path
    }) | Out-Null
}

$items | Sort-Object executablePath -Unique | ConvertTo-Json -Compress
