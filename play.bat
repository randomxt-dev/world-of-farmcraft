@echo off
rem World of Farmcraft: build (if needed) and play. Needs Rust and the WoW 1.12.1 data linked as WoW\.
cd /d "%~dp0"
cargo build --release -p farmcraft || (echo Build failed. & pause & exit /b 1)
start "" "target\release\farmcraft.exe"
