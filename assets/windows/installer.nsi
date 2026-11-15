; The Windows installer, made by scripts/package-windows.sh with NSIS (makensis), which passes
; APP_NAME, VERSION, STAGE (the folder of files to install), ICON and OUTFILE.
;
; Per-user, so no administrator rights: the app goes in %LOCALAPPDATA%\Programs, with a
; Start-menu shortcut and an uninstaller listed in Settings › Apps. FFmpeg is an optional
; component, ticked by default; someone with their own ffmpeg unticks it (docs/DISTRIBUTION.md
; §3). The app's settings and library database (%APPDATA%\<app id>) are the user's, and stay
; when it's uninstalled.

Unicode true
!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "Sections.nsh"
!include "FileFunc.nsh"

!define UNINST_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_NAME}"

Name "${APP_NAME}"
OutFile "${OUTFILE}"
InstallDir "$LOCALAPPDATA\Programs\${APP_NAME}"
InstallDirRegKey HKCU "${UNINST_KEY}" "InstallLocation"
RequestExecutionLevel user
SetCompressor /SOLID lzma
BrandingText "${APP_NAME} ${VERSION}"

VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "${APP_NAME}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "FileDescription" "${APP_NAME} installer"
VIAddVersionKey "LegalCopyright" "MIT licence"

!define MUI_ICON "${ICON}"
!define MUI_UNICON "${ICON}"
!define MUI_ABORTWARNING
!define MUI_FINISHPAGE_RUN "$INSTDIR\pausetape.exe"
!define MUI_FINISHPAGE_RUN_TEXT "Open ${APP_NAME}"

!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Section "${APP_NAME}" SecApp
    SectionIn RO
    SetOutPath "$INSTDIR"
    File "${STAGE}\pausetape.exe"
    SetOutPath "$INSTDIR\Licenses"
    File "${STAGE}\Licenses\LICENSE.txt"
    File "${STAGE}\Licenses\THIRD-PARTY.txt"
    SetOutPath "$INSTDIR\Licenses\Fonts"
    File "${STAGE}\Licenses\Fonts\*.txt"
    SetOutPath "$INSTDIR"
    WriteUninstaller "$INSTDIR\Uninstall.exe"
    CreateShortcut "$SMPROGRAMS\${APP_NAME}.lnk" "$INSTDIR\pausetape.exe"
    WriteRegStr HKCU "${UNINST_KEY}" "DisplayName" "${APP_NAME}"
    WriteRegStr HKCU "${UNINST_KEY}" "DisplayVersion" "${VERSION}"
    WriteRegStr HKCU "${UNINST_KEY}" "DisplayIcon" "$INSTDIR\pausetape.exe"
    WriteRegStr HKCU "${UNINST_KEY}" "InstallLocation" "$INSTDIR"
    WriteRegStr HKCU "${UNINST_KEY}" "UninstallString" '"$INSTDIR\Uninstall.exe"'
    WriteRegStr HKCU "${UNINST_KEY}" "QuietUninstallString" '"$INSTDIR\Uninstall.exe" /S'
    WriteRegDWORD HKCU "${UNINST_KEY}" "NoModify" 1
    WriteRegDWORD HKCU "${UNINST_KEY}" "NoRepair" 1
SectionEnd

Section "FFmpeg (conversion)" SecFfmpeg
    SetOutPath "$INSTDIR"
    File "${STAGE}\ffmpeg.exe"
    SetOutPath "$INSTDIR\Licenses\FFmpeg"
    File "${STAGE}\Licenses\FFmpeg\*.*"
SectionEnd

; Unticked when installing again: the copy from before goes too, so the app uses the user's own.
Section "-after"
    ${IfNot} ${SectionIsSelected} ${SecFfmpeg}
        Delete "$INSTDIR\ffmpeg.exe"
        RMDir /r "$INSTDIR\Licenses\FFmpeg"
    ${EndIf}
    ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
    IntFmt $0 "0x%08X" $0
    WriteRegDWORD HKCU "${UNINST_KEY}" "EstimatedSize" $0
SectionEnd

Section "Uninstall"
    Delete "$INSTDIR\pausetape.exe"
    Delete "$INSTDIR\ffmpeg.exe"
    RMDir /r "$INSTDIR\Licenses"
    Delete "$INSTDIR\Uninstall.exe"
    RMDir "$INSTDIR"
    Delete "$SMPROGRAMS\${APP_NAME}.lnk"
    DeleteRegKey HKCU "${UNINST_KEY}"
SectionEnd

!insertmacro MUI_FUNCTION_DESCRIPTION_BEGIN
    !insertmacro MUI_DESCRIPTION_TEXT ${SecApp} "The app."
    !insertmacro MUI_DESCRIPTION_TEXT ${SecFfmpeg} "Plays Dolby, Opus, APE and WMA files, cuts CUE albums into songs, and converts what the player can't read. About 6 MB.$\r$\n$\r$\nAlready have FFmpeg? Untick this to use yours."
!insertmacro MUI_FUNCTION_DESCRIPTION_END

; Installing again keeps an earlier choice to leave FFmpeg out.
Function .onInit
    ReadRegStr $0 HKCU "${UNINST_KEY}" "InstallLocation"
    ${If} $0 != ""
    ${AndIfNot} ${FileExists} "$0\ffmpeg.exe"
        !insertmacro UnselectSection ${SecFfmpeg}
    ${EndIf}
FunctionEnd
