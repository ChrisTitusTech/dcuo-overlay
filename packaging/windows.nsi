Unicode True
!include "MUI2.nsh"
!include "x64.nsh"
!ifndef VERSION
!define VERSION "0.1.1"
!endif
!ifndef BINARY
!define BINARY "..\target\release\dcuo-companion.exe"
!endif
Name "DCUO Companion"
OutFile "..\dist\DCUO-Companion-${VERSION}-windows-x86_64-setup.exe"
InstallDir "$LOCALAPPDATA\Programs\DCUO Companion"
InstallDirRegKey HKCU "Software\DCUO Companion" "InstallDir"
RequestExecutionLevel user
SetCompressor /SOLID lzma
!define MUI_ICON "dcuo-companion.ico"
!define MUI_UNICON "dcuo-companion.ico"
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_LICENSE "..\LICENSE"
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"
Function .onInit
  ${IfNot} ${RunningX64}
    MessageBox MB_ICONSTOP "DCUO Companion requires 64-bit Windows."
    Abort
  ${EndIf}
FunctionEnd
Section "DCUO Companion" SEC_MAIN
  SetShellVarContext current
  SetOutPath "$INSTDIR"
  File /oname=dcuo-companion.exe "${BINARY}"
  File "..\LICENSE"
  File "..\README.md"
  File "..\docs\release-0.1.md"
  File "..\docs\release-0.1.1.md"
  WriteUninstaller "$INSTDIR\Uninstall.exe"
  CreateDirectory "$SMPROGRAMS\DCUO Companion"
  CreateShortcut "$SMPROGRAMS\DCUO Companion\DCUO Companion.lnk" "$INSTDIR\dcuo-companion.exe"
  CreateShortcut "$SMPROGRAMS\DCUO Companion\Uninstall.lnk" "$INSTDIR\Uninstall.exe"
  WriteRegStr HKCU "Software\DCUO Companion" "InstallDir" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\DCUO Companion" "DisplayName" "DCUO Companion"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\DCUO Companion" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\DCUO Companion" "UninstallString" '$\"$INSTDIR\Uninstall.exe$\"'
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\DCUO Companion" "DisplayIcon" "$INSTDIR\dcuo-companion.exe"
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\DCUO Companion" "NoModify" 1
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\DCUO Companion" "NoRepair" 1
SectionEnd
Section "Uninstall"
  SetShellVarContext current
  Delete "$SMPROGRAMS\DCUO Companion\DCUO Companion.lnk"
  Delete "$SMPROGRAMS\DCUO Companion\Uninstall.lnk"
  RMDir "$SMPROGRAMS\DCUO Companion"
  Delete "$INSTDIR\dcuo-companion.exe"
  Delete "$INSTDIR\LICENSE"
  Delete "$INSTDIR\README.md"
  Delete "$INSTDIR\release-0.1.md"
  Delete "$INSTDIR\release-0.1.1.md"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\DCUO Companion"
  DeleteRegKey HKCU "Software\DCUO Companion"
  ; Personal settings and imported maps are intentionally preserved.
SectionEnd
