@echo off
cd /d "%~dp0"
if exist "target\release\rustmc.exe" (
    "target\release\rustmc.exe" %*
    exit /b
)
if exist "rustmc.exe" (
    "rustmc.exe" %*
    exit /b
)
if exist "target\release\rustmc-updated.exe" (
    "target\release\rustmc-updated.exe" %*
    exit /b
)
if not exist "target\release\rustmc.exe" (
    cargo build --release --offline --bin rustmc
    if errorlevel 1 (
        pause
        exit /b 1
    )
)
"target\release\rustmc.exe" %*
