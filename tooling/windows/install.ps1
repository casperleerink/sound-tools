# Installs Sound Tools for this user, in %LOCALAPPDATA%\Programs\Sound Tools, with a Start menu
# entry. Run it from the folder of the zip: right-click it, Run with PowerShell. Run it again to
# update. It also adds the folder to your PATH, for the sound-tools command. To remove Sound
# Tools, delete the two paths it prints and take the folder off your PATH.
#
# Plain ASCII only: Windows PowerShell reads a file without a byte order mark as ANSI.
$ErrorActionPreference = 'Stop'

$folder = Join-Path $env:LOCALAPPDATA 'Programs\Sound Tools'
$program = Join-Path $folder 'sound-tools.exe'
$old = "$program.old"
$shortcut = Join-Path ([Environment]::GetFolderPath('Programs')) 'Sound Tools.lnk'

try {
    New-Item -ItemType Directory -Force -Path $folder | Out-Null
    # Windows cannot overwrite a running program but can rename it, so a running Sound Tools
    # does not stop the update. The app removes the old one at its next start.
    # The copy goes next to it first, so a copy that stops halfway leaves the old program whole.
    # The swap is then two renames.
    $new = "$program.new"
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'sound-tools.exe') -Destination $new -Force
    if (Test-Path -LiteralPath $old) {
        Remove-Item -LiteralPath $old -Force
    }
    if (Test-Path -LiteralPath $program) {
        Move-Item -LiteralPath $program -Destination $old
    }
    try {
        Move-Item -LiteralPath $new -Destination $program
    } catch {
        if (Test-Path -LiteralPath $old) {
            Move-Item -LiteralPath $old -Destination $program
        }
        throw
    }

    $link = (New-Object -ComObject WScript.Shell).CreateShortcut($shortcut)
    $link.TargetPath = $program
    $link.WorkingDirectory = $folder
    $link.Save()

    # The sound-tools command for terminals and agents, as install.sh puts it in ~/.local/bin.
    # A new terminal sees it; one open already does not.
    $path = [Environment]::GetEnvironmentVariable('Path', 'User')
    $entries = @($path -split ';' | Where-Object { $_ })
    if ($entries -notcontains $folder) {
        [Environment]::SetEnvironmentVariable('Path', (($entries + $folder) -join ';'), 'User')
    }

    Write-Output 'Installed Sound Tools:'
    Write-Output "  $folder"
    Write-Output "  $shortcut"
} finally {
    # Run with PowerShell closes the window as soon as the script ends, success or error. The
    # app's own update runs this with -NonInteractive, where nobody reads it.
    if (-not ([Environment]::GetCommandLineArgs() -contains '-NonInteractive')) {
        Read-Host 'Press Enter to close' | Out-Null
    }
}
