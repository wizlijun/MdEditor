; A `notemd` command on Windows.
;
; The GUI executable IS notemd.exe, and cmd's PATHEXT puts .EXE ahead of .CMD —
; so a shim sitting beside it would be shadowed and `notemd search` would open a
; window instead of printing results. Hence a `bin\` subdirectory: only that
; goes on PATH, the GUI executable never does, and the command is spelled the
; same on macOS and Windows — which is the point, because AGENTS.md tells every
; agent one command, not two.
;
; NOT VERIFIED ON WINDOWS. This file was written and reasoned through on macOS,
; where NSIS cannot run. See task-15-report.md for exactly what a human must
; check on a real Windows box before this ships: fresh install (bin\notemd.cmd
; exists, `notemd search` works from a new shell), upgrade/repair (PATH gains
; the entry exactly once), and uninstall (the .cmd, the now-empty bin\, and the
; PATH entry are all gone).
;
; No plugins and no extra !include beyond what stock NSIS ships. `${StrContains}`
; is NOT built into NSIS (it needs StrFunc.nsh's init dance, which is easy to
; get subtly wrong and impossible to iterate on here) — instead this file
; defines its own tiny substring-index function, `NotemdStrIndexOf`, adapted
; from the long-standing plain-NSIS "does haystack contain needle" recipe
; (Push/Exch/Pop register-preserving calling convention, index-scanning loop).
; It is instantiated twice, once per NSIS's own rule that a Function called
; from the uninstaller must be named `un.<name>`.

Var R_SIO_Haystack
Var R_SIO_Needle
Var R_SIO_Idx
Var R_SIO_NeedleLen
Var R_SIO_HayLen
Var R_SIO_Slice
Var R_SIO_Result

Var NotemdFileHandle
Var NotemdBin
Var NotemdPath
Var NotemdNeedle
Var NotemdNeedleLen
Var NotemdIdx
Var NotemdTailStart
Var NotemdLeft
Var NotemdRight
Var NotemdCheckSlice

; ---------------------------------------------------------------------------
; NotemdStrIndexOf / un.NotemdStrIndexOf
;
; Call convention: Push haystack, Push needle (needle on top), Call, Pop result.
; Result is the 0-based index of the first occurrence of needle in haystack,
; or "" if it is not present. Case-sensitive plain character comparison only
; — no locale/case folding, which is fine here: both sides of every call in
; this file are strings this same installer wrote itself.
; ---------------------------------------------------------------------------
!macro NotemdStrIndexOfImpl un
Function ${un}NotemdStrIndexOf
  Exch $R_SIO_Needle
  Exch
  Exch $R_SIO_Haystack
  StrCpy $R_SIO_Result ""
  StrCpy $R_SIO_Idx -1
  StrLen $R_SIO_NeedleLen $R_SIO_Needle
  StrLen $R_SIO_HayLen $R_SIO_Haystack
  loop:
    IntOp $R_SIO_Idx $R_SIO_Idx + 1
    StrCpy $R_SIO_Slice $R_SIO_Haystack $R_SIO_NeedleLen $R_SIO_Idx
    StrCmp $R_SIO_Slice $R_SIO_Needle found
    StrCmp $R_SIO_Idx $R_SIO_HayLen done
    Goto loop
  found:
    StrCpy $R_SIO_Result $R_SIO_Idx
  done:
  Pop $R_SIO_Needle
  Exch $R_SIO_Result
FunctionEnd
!macroend

!insertmacro NotemdStrIndexOfImpl ""
!insertmacro NotemdStrIndexOfImpl "un."

; ---------------------------------------------------------------------------
; un.NotemdRemoveFromPath — the mirror image of the append below. Expects
; $NotemdBin already set to "$INSTDIR\bin". Handles all three shapes our own
; installer can produce (sole entry / trailing entry / entry followed by more
; after a manual PATH reorder); if the entry is not found at all, leaves the
; registry value untouched.
; ---------------------------------------------------------------------------
Function un.NotemdRemoveFromPath
  ReadRegStr $NotemdPath HKCU "Environment" "Path"
  StrCmp $NotemdPath "" notemd_rm_done

  ; Case: our bin dir is the entire PATH value (nothing else was ever there).
  StrCmp $NotemdPath $NotemdBin notemd_rm_only notemd_rm_check_mid

  notemd_rm_only:
    StrCpy $NotemdPath ""
    Goto notemd_rm_write

  notemd_rm_check_mid:
    ; Case: "...;$INSTDIR\bin" — mid-list or trailing. This is the shape the
    ; installer itself always writes (append after a semicolon), so it is the
    ; expected case on an ordinary uninstall.
    StrCpy $NotemdNeedle ";$NotemdBin"
    Push $NotemdPath
    Push $NotemdNeedle
    Call un.NotemdStrIndexOf
    Pop $NotemdIdx
    StrCmp $NotemdIdx "" notemd_rm_check_prefix notemd_rm_do_mid

  notemd_rm_do_mid:
    StrLen $NotemdNeedleLen $NotemdNeedle
    StrCpy $NotemdLeft $NotemdPath $NotemdIdx
    IntOp $NotemdTailStart $NotemdIdx + $NotemdNeedleLen
    StrCpy $NotemdRight $NotemdPath "" $NotemdTailStart
    StrCpy $NotemdPath "$NotemdLeft$NotemdRight"
    Goto notemd_rm_write

  notemd_rm_check_prefix:
    ; Case: "$INSTDIR\bin;..." at the very start — only reachable if the user
    ; manually reordered PATH so nothing precedes our entry.
    StrLen $NotemdNeedleLen $NotemdBin
    IntOp $NotemdTailStart $NotemdNeedleLen + 1
    StrCpy $NotemdCheckSlice $NotemdPath $NotemdTailStart
    StrCpy $NotemdNeedle "$NotemdBin;"
    StrCmp $NotemdCheckSlice $NotemdNeedle notemd_rm_do_prefix notemd_rm_done

  notemd_rm_do_prefix:
    StrCpy $NotemdPath $NotemdPath "" $NotemdTailStart
    Goto notemd_rm_write

  notemd_rm_write:
    WriteRegExpandStr HKCU "Environment" "Path" "$NotemdPath"
    ; Broadcast the change so already-open shells/Explorer notice it without a
    ; logoff. 0xFFFF/0x1A are HWND_BROADCAST/WM_WININICHANGE — hardcoded
    ; rather than pulled from WinMessages.nsh so this file has zero !include
    ; surface (a second, conflicting include of that header elsewhere in
    ; Tauri's generated installer.nsi would be a hard compile error).
    SendMessage 0xFFFF 0x1A 0 "STR:Environment" /TIMEOUT=5000

  notemd_rm_done:
FunctionEnd

!macro NSIS_HOOK_POSTINSTALL
  CreateDirectory "$INSTDIR\bin"
  FileOpen $NotemdFileHandle "$INSTDIR\bin\notemd.cmd" w
  FileWrite $NotemdFileHandle '@"%~dp0..\notemd.exe" --cli %*$\r$\n'
  FileWrite $NotemdFileHandle '@exit /b %ERRORLEVEL%$\r$\n'
  FileClose $NotemdFileHandle

  ; Append $INSTDIR\bin to the *user* PATH (HKCU), never the machine PATH —
  ; this is a per-user install, and rewriting the system PATH from an
  ; installer is how PATHs get destroyed. Guarded so upgrades/repairs never
  ; duplicate the entry: an unconditional append would grow the user's PATH
  ; by one entry on every single update until it breaks.
  StrCpy $NotemdBin "$INSTDIR\bin"
  ReadRegStr $NotemdPath HKCU "Environment" "Path"
  Push $NotemdPath
  Push $NotemdBin
  Call NotemdStrIndexOf
  Pop $NotemdIdx
  StrCmp $NotemdIdx "" notemd_path_append notemd_path_done

  notemd_path_append:
    StrCmp $NotemdPath "" notemd_path_set_bare notemd_path_append_sep
    notemd_path_set_bare:
      StrCpy $NotemdPath "$NotemdBin"
      Goto notemd_path_write
    notemd_path_append_sep:
      StrCpy $NotemdPath "$NotemdPath;$NotemdBin"
    notemd_path_write:
    WriteRegExpandStr HKCU "Environment" "Path" "$NotemdPath"
    SendMessage 0xFFFF 0x1A 0 "STR:Environment" /TIMEOUT=5000

  notemd_path_done:
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  Delete "$INSTDIR\bin\notemd.cmd"
  ; No /r: this only succeeds if bin\ is empty. If a user dropped their own
  ; files in there, that is not ours to delete, and we leave the directory.
  RMDir "$INSTDIR\bin"

  StrCpy $NotemdBin "$INSTDIR\bin"
  Call un.NotemdRemoveFromPath
!macroend
