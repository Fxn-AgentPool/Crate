@echo off
rem Builds Crate in release mode: target\release\crate.exe
rem   build.bat         build
rem   build.bat run     build, then start Crate
rem   build.bat debug   debug build (target\debug\crate.exe)
setlocal
cd /d "%~dp0"

where cargo >nul 2>nul || set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
where cargo >nul 2>nul || (
    echo cargo not found. Install Rust from https://rustup.rs
    exit /b 1
)

set "PROFILE=--release"
set "OUT=target\release\crate.exe"
if /i "%~1"=="debug" (
    set "PROFILE="
    set "OUT=target\debug\crate.exe"
)

tasklist /fi "imagename eq crate.exe" 2>nul | find /i "crate.exe" >nul && (
    echo Crate is running. Close it first: Windows cannot replace a running exe.
    exit /b 1
)

cargo build %PROFILE% || exit /b 1
echo Built %OUT%

if /i "%~1"=="run" start "" "%OUT%"
endlocal
