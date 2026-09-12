@echo off
rem  EQL Grimoire — double-click this.
rem
rem  Builds the workbench if it needs building, starts the engine, opens the app.
rem  Everything runs on this machine: nothing is uploaded, nothing is hosted, and the
rem  only thing listening is 127.0.0.1.

setlocal
cd /d "%~dp0"

where cargo >nul 2>&1
if errorlevel 1 (
  echo.
  echo   Rust is not on PATH. Install it from https://rustup.rs and run this again.
  echo.
  pause
  exit /b 1
)

if not exist "web\corpus.grim" (
  echo.
  echo   web\corpus.grim is missing. Cut one with:
  echo     cargo run --release -p grimoire-forge -- corpus web\corpus.grim --from data
  echo.
  pause
  exit /b 1
)

echo   Building...
cargo build --release -p grimoire-forge
if errorlevel 1 (
  echo.
  echo   Build failed. The error is above.
  echo.
  pause
  exit /b 1
)

echo.
echo   EQL Grimoire is at  http://127.0.0.1:8787/app.html
echo   Close this window to stop it.
echo.

rem Give the listener a moment before the browser asks for the page.
start "" /b cmd /c "timeout /t 2 >nul & start "" http://127.0.0.1:8787/app.html"
target\release\grimoire.exe serve --root web --corpus web\corpus.grim

endlocal
