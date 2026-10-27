@echo off
rem Serve docs\ at http://localhost:8642 (if it isn't already) and open the design page in Chrome.
powershell -NoProfile -Command "try { Invoke-WebRequest -UseBasicParsing http://localhost:8642/design.html -TimeoutSec 2 | Out-Null } catch { Start-Process python -ArgumentList '-m','http.server','8642','--bind','127.0.0.1','--directory','%~dp0docs' -WindowStyle Hidden; Start-Sleep 2 }"
start chrome http://localhost:8642/design.html
