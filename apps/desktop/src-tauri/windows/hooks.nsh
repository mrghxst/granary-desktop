; Granary NSIS hooks (ADR-0005). Tauri's "Delete the application data" checkbox in the uninstaller removes
; %APPDATA% and %LOCALAPPDATA% folders named after the bundle identifier (WebView2 data, window state). Granary keeps
; its own data under the product name (apps/desktop/src-tauri/src/paths.rs), so remove that too when the user asked for
; it. A silent uninstall (/S) leaves the box unticked and keeps all user data.
; The per-user install directory is also %LOCALAPPDATA%\Granary: the uninstaller has already deleted the program files
; it installed, and only the folders below are user data. The display restore marker (display-restore.json) is kept on
; purpose: it only exists if a display change still needs repairing at the next start.
; Refresh tokens live in Windows Credential Manager (service "Granary"); removing a server in the app
; deletes its token, the uninstaller does not touch Credential Manager.
; Tauri's template keeps the install-location key (HKCU\Software\<publisher>\<product>) unless app data is deleted;
; it is not user data, so a real uninstall (not an update) always removes it (phase-9 review M2).

!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $UpdateMode <> 1
    DeleteRegKey SHCTX "${MANUPRODUCTKEY}"
    DeleteRegKey /ifempty SHCTX "${MANUKEY}"
  ${EndIf}
  ${If} $DeleteAppDataCheckboxState = 1
  ${AndIf} $UpdateMode <> 1
    SetShellVarContext current
    RmDir /r "$APPDATA\Granary"
    RmDir /r "$LOCALAPPDATA\Granary\logs"
    RmDir /r "$LOCALAPPDATA\Granary\shader-cache"
    RMDir "$LOCALAPPDATA\Granary"
  ${EndIf}
!macroend
