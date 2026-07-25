; Sim Display Fixer installer
; Per-user install (no admin needed to install). The app itself elevates only when it needs to
; register the auto-fix task or restart the display adapter.

Unicode true
!include "MUI2.nsh"
!include "FileFunc.nsh"

!ifndef VERSION
  !define VERSION "0.2.2"
!endif

Name "Sim Display Fixer"
OutFile "..\target\packaged\SimDisplayFixer-${VERSION}-setup.exe"
InstallDir "$LOCALAPPDATA\Sim Display Fixer"
InstallDirRegKey HKCU "Software\Lunis\SimDisplayFixer" "InstallDir"
RequestExecutionLevel user
SetCompressor /SOLID lzma

VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "Sim Display Fixer"
VIAddVersionKey "CompanyName" "Lunis"
VIAddVersionKey "FileDescription" "Sim Display Fixer Setup"
VIAddVersionKey "FileVersion" "${VERSION}.0"
VIAddVersionKey "ProductVersion" "${VERSION}.0"
VIAddVersionKey "LegalCopyright" "Copyright (c) 2026 Lunis"

!define MUI_ICON "..\app\icons\icon.ico"
!define MUI_UNICON "..\app\icons\icon.ico"
!define MUI_ABORTWARNING

!insertmacro MUI_PAGE_LICENSE "..\LICENSE.md"
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!define MUI_FINISHPAGE_RUN "$INSTDIR\sim-display-fixer.exe"
!define MUI_FINISHPAGE_RUN_TEXT "Open Sim Display Fixer"
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"

; Install a TTF for the current user and tell Windows about it.
!macro InstallFont FILE NAME
  SetOutPath "$LOCALAPPDATA\Microsoft\Windows\Fonts"
  File "..\app\fonts\${FILE}"
  WriteRegStr HKCU "Software\Microsoft\Windows NT\CurrentVersion\Fonts" "${NAME}" \
    "$LOCALAPPDATA\Microsoft\Windows\Fonts\${FILE}"
!macroend

Section "Install"
  ; stop a running copy so the exe can be replaced
  nsExec::Exec 'taskkill /F /IM sim-display-fixer.exe'
  Pop $0

  SetOutPath "$INSTDIR"
  File "..\target\release\sim-display-fixer.exe"
  File "..\LICENSE.md"

  ; DM Sans - the UI asks for it by name, so it must be present on the system
  !insertmacro InstallFont "DMSans-Regular.ttf" "DM Sans (TrueType)"
  !insertmacro InstallFont "DMSans-Medium.ttf" "DM Sans Medium (TrueType)"
  !insertmacro InstallFont "DMSans-SemiBold.ttf" "DM Sans SemiBold (TrueType)"
  SetOutPath "$INSTDIR"

  CreateShortCut "$SMPROGRAMS\Sim Display Fixer.lnk" "$INSTDIR\sim-display-fixer.exe"

  WriteRegStr HKCU "Software\Lunis\SimDisplayFixer" "InstallDir" "$INSTDIR"
  WriteUninstaller "$INSTDIR\uninstall.exe"

  ; Add/Remove Programs entry
  !define UNINST_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\SimDisplayFixer"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayName" "Sim Display Fixer"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINST_KEY}" "Publisher" "Lunis"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayIcon" "$INSTDIR\sim-display-fixer.exe"
  WriteRegStr HKCU "${UNINST_KEY}" "UninstallString" "$INSTDIR\uninstall.exe"
  WriteRegStr HKCU "${UNINST_KEY}" "URLInfoAbout" "https://lunis.live"
  WriteRegDWORD HKCU "${UNINST_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINST_KEY}" "NoRepair" 1
  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  IntFmt $0 "0x%08X" $0
  WriteRegDWORD HKCU "${UNINST_KEY}" "EstimatedSize" "$0"
SectionEnd

Section "Uninstall"
  nsExec::Exec 'taskkill /F /IM sim-display-fixer.exe'
  Pop $0

  ; remove the auto-fix task and the hidden-startup entry we may have created
  nsExec::Exec 'schtasks /Delete /TN "SimDisplayFixer" /F'
  Pop $0
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "SimDisplayFixer"

  Delete "$INSTDIR\sim-display-fixer.exe"
  Delete "$INSTDIR\LICENSE.md"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"

  Delete "$SMPROGRAMS\Sim Display Fixer.lnk"

  DeleteRegKey HKCU "Software\Lunis\SimDisplayFixer"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\SimDisplayFixer"

  ; the saved display target and log
  Delete "$APPDATA\..\..\ProgramData\Lunis\DisplayFixer\profile.bin"
SectionEnd
