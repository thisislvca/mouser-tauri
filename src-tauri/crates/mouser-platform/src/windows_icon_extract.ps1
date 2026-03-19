$source = $args[0]
if ([string]::IsNullOrWhiteSpace($source) -or -not (Test-Path -LiteralPath $source)) {
  return
}

try {
  Add-Type -AssemblyName System.Drawing
} catch {
}

try {
  $icon = [System.Drawing.Icon]::ExtractAssociatedIcon($source)
  if ($null -eq $icon) {
    return
  }

  $bitmap = $icon.ToBitmap()
  $stream = New-Object System.IO.MemoryStream

  try {
    $bitmap.Save($stream, [System.Drawing.Imaging.ImageFormat]::Png)
    [Convert]::ToBase64String($stream.ToArray())
  } finally {
    $stream.Dispose()
    $bitmap.Dispose()
    $icon.Dispose()
  }
} catch {
}
