@echo off
rem Starts Calm Days. Then open http://127.0.0.1:3000 in your browser (this computer only).
rem The first build takes a few minutes. Do not close the window while it builds.
cd /d "%~dp0"
rem The AI reader needs Ollama. Start it if it is not already running (the app also tries this itself).
curl -s -m 2 http://127.0.0.1:11434/api/tags >nul 2>&1
if errorlevel 1 (
  where ollama >nul 2>&1 && start "" /b ollama serve
)
cargo run -j 2
pause
