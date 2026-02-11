@echo off
:: Build-Skript fuer OpenWilly Release
:: Erstellt eine verteilbare Release-Version

echo.
echo  =============================================
echo   OpenWilly Release Build
echo  =============================================
echo.

cd /d "%~dp0\.."

:: Release-Build
echo  [1/3] Kompiliere Release-Build...
cargo build --release -p openwilly-player
if errorlevel 1 (
    echo  FEHLER: Build fehlgeschlagen!
    pause
    exit /b 1
)

:: Kopiere Dateien in release/
echo  [2/3] Erstelle Release-Ordner...
if not exist "release" mkdir release

:: Kopiere Executable
copy /y "target\release\openwilly.exe" "release\openwilly.exe" >nul 2>&1
if errorlevel 1 (
    echo  WARNUNG: openwilly.exe nicht in target\release gefunden.
    echo  Pruefe ob der Build korrekt war.
)

:: Stelle sicher, dass start.bat und README.txt vorhanden sind
if not exist "release\start.bat" (
    echo  WARNUNG: release\start.bat fehlt!
)
if not exist "release\README.txt" (
    echo  WARNUNG: release\README.txt fehlt!
)

echo  [3/3] Fertig!
echo.
echo  Release-Ordner: %cd%\release\
echo.
echo  Inhalt:
dir /b release\
echo.
echo  Zum Spielen: Lege eine Willy-Werkel-ISO in den
echo  release/ Ordner und starte start.bat
echo.
pause
