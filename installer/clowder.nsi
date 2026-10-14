; Clowder 安装包（NSIS 3）
; 用法（CI / 本地都走这条）:  makensis /DVERSION=0.1.0 installer/clowder.nsi
;   需要先 cargo build --release --locked 产出 target\release\clowder.exe

!define APP_NAME "Clowder"
!define APP_EXE  "clowder.exe"
!define PUBLISHER "hao"

Unicode true
ManifestDPIAware true
RequestExecutionLevel user        ; 默认 per-user，不弹 UAC

; File / File /r 的相对路径相对这里；从 installer/ 跳回仓库根，CI 和本地都适用
!define REPO_ROOT ".."
SetCompressor /SOLID lzma

; MultiUser 的 define 必须在 !include MultiUser.nsh 之前:
;   Highest         → 普通用户默认 per-user，管理员可选 all-users
;   COMMANDLINE     → 支持 /AllUsers /CurrentUser 静默切换
;   INSTDIR         → Program Files / LocalAppData 下的子目录名
!define MULTIUSER_EXECUTIONLEVEL Highest
!define MULTIUSER_MUI
!define MULTIUSER_INSTALLMODE_COMMANDLINE
!define MULTIUSER_USE_PROGRAMFILES64
!define MULTIUSER_INSTALLMODE_INSTDIR "${APP_NAME}"
; 卸载时从注册表回读出安装模式（per-user / all-users）→ SHCTX 落同一位置
!define MULTIUSER_INSTALLMODE_DEFAULT_REGISTRY_KEY      "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_NAME}"
!define MULTIUSER_INSTALLMODE_DEFAULT_REGISTRY_VALUENAME "InstallMode"
; 首屏默认选中 per-user（即使你是管理员）
!define MULTIUSER_INSTALLMODE_DEFAULT_CURRENTUSER

!include "MUI2.nsh"
!include "MultiUser.nsh"
!include "FileFunc.nsh"

Name "${APP_NAME}"
OutFile "${REPO_ROOT}\clowder-${VERSION}-setup.exe"

; 给编译期一个占位 INSTDIR；真正路径由 MULTIUSER_INIT 在运行时根据 per-user/all-users 改写
InstallDir "$LOCALAPPDATA\Programs\${APP_NAME}"

; 首屏问 per-user / all-users
!insertmacro MULTIUSER_PAGE_INSTALLMODE

; ——————————————————————————————————————————
; 页面
; ——————————————————————————————————————————
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES

!define MUI_FINISHPAGE_RUN "$INSTDIR\${APP_EXE}"
!define MUI_FINISHPAGE_RUN_TEXT "启动 ${APP_NAME}"
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"
; 中文用户系统会弹英文界面；后续想要中文界面再加一行:
; !insertmacro MUI_LANGUAGE "SimpChinese"

Function .onInit
  !insertmacro MULTIUSER_INIT
FunctionEnd

Function un.onInit
  !insertmacro MULTIUSER_UNINIT
FunctionEnd

; ——————————————————————————————————————————
; 安装
; ——————————————————————————————————————————
Section "!${APP_NAME}" SEC01
  SetOutPath $INSTDIR
  File "${REPO_ROOT}\target\release\${APP_EXE}"

  ; 运行时资源：exe 同级 assets\（src/app.rs:149 font_dirs 会命中）
  SetOutPath $INSTDIR\assets
  File /r "${REPO_ROOT}\assets\*.*"

  WriteUninstaller "$INSTDIR\uninstall.exe"

  ; 开始菜单
  CreateDirectory "$SMPROGRAMS\${APP_NAME}"
  CreateShortcut  "$SMPROGRAMS\${APP_NAME}\${APP_NAME}.lnk" "$INSTDIR\${APP_EXE}"
  CreateShortcut  "$SMPROGRAMS\${APP_NAME}\Uninstall.lnk" "$INSTDIR\uninstall.exe"

  ; 桌面快捷方式
  CreateShortcut "$DESKTOP\${APP_NAME}.lnk" "$INSTDIR\${APP_EXE}"

  ; “应用和功能”列表
  ; 记录 InstallMode 让 MULTIUSER_UNINIT 回读出 SHCTX 的正确落点
  WriteRegStr SHCTX "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_NAME}" "InstallMode"     "$MultiUser.InstallMode"
  WriteRegStr SHCTX "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_NAME}" "DisplayName"     "${APP_NAME}"
  WriteRegStr SHCTX "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_NAME}" "DisplayVersion"  "${VERSION}"
  WriteRegStr SHCTX "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_NAME}" "Publisher"       "${PUBLISHER}"
  WriteRegStr SHCTX "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_NAME}" "UninstallString" "$INSTDIR\uninstall.exe"
  WriteRegDWORD SHCTX "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_NAME}" "NoModify" 1
  WriteRegDWORD SHCTX "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_NAME}" "NoRepair" 1
  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  IntFmt $0 "0x%08X" $0
  WriteRegDWORD SHCTX "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_NAME}" "EstimatedSize" "$0"
SectionEnd

; ——————————————————————————————————————————
; 卸载
; ——————————————————————————————————————————
Section "Uninstall"
  Delete "$DESKTOP\${APP_NAME}.lnk"
  Delete "$SMPROGRAMS\${APP_NAME}\*.lnk"
  RMDir  "$SMPROGRAMS\${APP_NAME}"
  RMDir /r "$INSTDIR"
  DeleteRegKey SHCTX "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_NAME}"
SectionEnd
