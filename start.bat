@echo off
rem Starts Calm Days. Then open http://127.0.0.1:3000 in your browser (this computer only).
rem The first build takes a few minutes. Do not close the window while it builds.
cd /d "%~dp0"
cargo run -j 2
pause
