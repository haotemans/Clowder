param([string]$Label = 'BASE', [string]$Envs = '', [int]$Max = 180, [int]$Step = 5)

$exe = "D:\weiai\project\Clowder\target\release\clowder.exe"
$out = "D:\weiai\project\Clowder\scripts\memlogs"
New-Item -ItemType Directory -Force -Path $out | Out-Null

Remove-Item Env:CLOWDER_MEM_NO_FONT, Env:CLOWDER_MEM_NO_PRICING, Env:CLOWDER_MEM_NO_PROXY, Env:CLOWDER_MEM_WORKERS, Env:CLOWDER_MEM_FONT, Env:CLOWDER_MEM_FONT_FILE, Env:CLOWDER_MEM_SERIF_FILE -ErrorAction SilentlyContinue
foreach ($e in ($Envs -split ';' | Where-Object { $_ })) {
  $k, $v = $e -split '=', 2
  Set-Item "Env:$k" $v
}

$so = Join-Path $out "$Label.out.txt"; $se = Join-Path $out "$Label.err.txt"
$p = Start-Process -FilePath $exe -PassThru -RedirectStandardOutput $so -RedirectStandardError $se
$sw = [Diagnostics.Stopwatch]::StartNew()
$i = 0
while (-not $p.HasExited -and $sw.Elapsed.TotalSeconds -lt $Max) {
  Start-Sleep -Seconds $Step
  $i++
  try { $q = Get-Process -Id $p.Id -ErrorAction Stop } catch { break }
  '{0} t={1,3}s ws={2,7}MB priv={3,7}MB peakWS={4,7}MB thr={5} cpu={6}s' -f $Label, [math]::Round($sw.Elapsed.TotalSeconds), [math]::Round($q.WorkingSet64 / 1MB, 1), [math]::Round($q.PrivateMemorySize64 / 1MB, 1), [math]::Round($q.PeakWorkingSet64 / 1MB, 1), $q.Threads.Count, [math]::Round($q.CPU, 2) | Write-Host
}
if ($p.HasExited) {
  Write-Host ("{0} EXITED at {1}s code={2}" -f $Label, [math]::Round($sw.Elapsed.TotalSeconds), $p.ExitCode)
  Write-Host ("  stderr tail: " + ((Get-Content $se -Raw) -replace '\s+', ' '))
} else {
  Write-Host ("{0} still running after {1}s -> killed" -f $Label, $Max)
  Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
}
