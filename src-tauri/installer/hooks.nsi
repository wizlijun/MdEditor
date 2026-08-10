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
; the entry exactly once), uninstall (the .cmd, the now-empty bin\, and the
; PATH entry are all gone), a per-user PATH longer than 1023 characters
; (PATH must come back byte-identical, both on install and on uninstall —
; see the READ FAILURES note below), and a *cancelled* uninstall (say "no" to
; the "app is still running" prompt: the PATH entry must survive).
;
; No plugins and no extra !include beyond what stock NSIS ships. `${StrContains}`
; is NOT built into NSIS (it needs StrFunc.nsh's init dance, which is easy to
; get subtly wrong and impossible to iterate on here) — instead this file
; defines its own tiny substring-index function, `NotemdStrIndexOf`, adapted
; from the long-standing plain-NSIS "does haystack contain needle" recipe
; (Push/Exch/Pop register-preserving calling convention, index-scanning loop).
; It is instantiated twice, once per NSIS's own rule that a Function called
; from the uninstaller must be named `un.<name>`.
;
; Every PATH check below is anchored on BOTH sides by the `;` separator (or
; string start/end) before it is trusted. Review round 1 found that an
; earlier version of this file searched for the left-anchored-only
; ";$INSTDIR\bin" and treated wherever that occurred as authoritative — which
; also matches inside an unrelated neighboring entry like "...\bin2", and on
; uninstall would silently splice that neighbor's name in half. See
; `NotemdPathHasEntry` and `un.NotemdRemoveFromPath` below and the fix-round-1
; trace table in task-15-report.md.
;
; READ FAILURES ARE NOT "VALUE ABSENT". `ReadRegStr` reads into a fixed
; `NSIS_MAX_STRLEN` buffer (1023 chars in stock makensis) and, when the stored
; value is longer than that — or the read fails for any other reason — sets
; the error flag and yields "". A per-user PATH over that length is unusual
; but entirely real. Treating that "" as "there is no PATH yet" would make the
; installer overwrite the user's entire per-user PATH with a single entry,
; silently, and again on every auto-update. So every `ReadRegStr` here is
; bracketed by `ClearErrors`/`IfErrors`, and on error `NotemdPathValueExists`
; decides between the two cases the empty string conflates:
;   * no `Path` value under HKCU\Environment at all → safe to create ours;
;   * a `Path` value exists but we could not read it → write NOTHING and bail.
; `EnumRegValue` is what makes that distinction possible: it reports value
; *names* without reading their contents, so it is unaffected by the length
; limit that defeated the read.
;
; (A PowerShell delegation — `[Environment]::SetEnvironmentVariable(...,'User')`
; — has no length limit and broadcasts WM_SETTINGCHANGE itself, and was
; considered. Rejected: it would mean re-expressing the PATH splicing in a
; second language, and that splicing is the part of this file that has been
; hand-verified across nine shapes including the `bin`/`bin2` prefix
; collision. Keeping one implementation of the splice and hardening the read
; around it disturbs less than replacing the whole thing with untested
; PowerShell that cannot be compile-checked here at all.)

Var R_SIO_Haystack
Var R_SIO_Needle
Var R_SIO_Idx
Var R_SIO_NeedleLen
Var R_SIO_HayLen
Var R_SIO_Slice
Var R_SIO_Result

Var PHE_Haystack
Var PHE_Needle
Var PHE_Result
Var PHE_NeedleLen
Var PHE_HayLen
Var PHE_Slice
Var PHE_Compound
Var PHE_CompoundLen
Var PHE_Idx
Var PHE_Offset

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
Var NotemdHasEntry
Var NotemdHayLen
Var NotemdOffset

Var NotemdPathValueFound
Var NotemdEnumIdx
Var NotemdEnumName

; ---------------------------------------------------------------------------
; NotemdStrIndexOf / un.NotemdStrIndexOf
;
; Call convention: Push haystack, Push needle (needle on top), Call, Pop result.
; Result is the 0-based index of the first occurrence of needle in haystack,
; or "" if it is not present. Case-sensitive plain character comparison only
; — no locale/case folding, which is fine here: both sides of every call in
; this file are strings this same installer wrote itself. This is a raw
; substring search with NO boundary awareness of its own — callers below are
; responsible for anchoring (see `NotemdPathHasEntry` / `un.NotemdRemoveFromPath`),
; which is exactly the responsibility review round 1 found missing.
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
; NotemdPathValueExists / un.NotemdPathValueExists
;
; Sets $NotemdPathValueFound to "1" if HKCU\Environment has a value *named*
; "Path", "" otherwise. Uses no stack — the caller reads the variable — so
; there is no Exch/Pop ordering to get wrong for a function whose entire job
; is to keep a failed read from being mistaken for an absent value.
;
; `EnumRegValue` enumerates names only, so a value too long for
; `NSIS_MAX_STRLEN` (the very case this exists to detect) is still reported.
; `StrCmp` is NSIS's case-INsensitive comparison, which is the right
; semantics here: registry value names are case-insensitive, and PATH is
; spelled "Path" or "PATH" depending on who wrote it last.
;
; Labels are function-scoped in NSIS, so the two instantiations below do not
; collide — same pattern as NotemdStrIndexOfImpl above.
; ---------------------------------------------------------------------------
!macro NotemdPathValueExistsImpl un
Function ${un}NotemdPathValueExists
  StrCpy $NotemdPathValueFound ""
  StrCpy $NotemdEnumIdx 0
  pve_loop:
    ClearErrors
    EnumRegValue $NotemdEnumName HKCU "Environment" $NotemdEnumIdx
    IfErrors pve_done
    StrCmp $NotemdEnumName "" pve_done
    StrCmp $NotemdEnumName "Path" pve_found
    IntOp $NotemdEnumIdx $NotemdEnumIdx + 1
    Goto pve_loop
  pve_found:
    StrCpy $NotemdPathValueFound "1"
  pve_done:
FunctionEnd
!macroend

!insertmacro NotemdPathValueExistsImpl ""
!insertmacro NotemdPathValueExistsImpl "un."

; ---------------------------------------------------------------------------
; NotemdPathHasEntry — is $INSTDIR\bin present as a *complete* PATH entry
; (not merely a substring of some other entry) in the given PATH value?
; Install-side only: the dedup check runs in NSIS_HOOK_POSTINSTALL, never in
; the uninstaller.
;
; Anchored on both sides for every shape: exact sole entry, leading
; ("bin;..."), middle/followed-by-more ("...;bin;..."), and trailing
; ("...;bin" at the true end, checked as a direct suffix comparison — not a
; search — so there is no ambiguity about *which* occurrence is "the end").
;
; Call convention: Push haystack, Push needle, Call, Pop "1" (present) or ""
; (absent).
; ---------------------------------------------------------------------------
Function NotemdPathHasEntry
  Exch $PHE_Needle
  Exch
  Exch $PHE_Haystack
  StrCpy $PHE_Result ""
  StrCmp $PHE_Haystack "" phe_done phe_continue

  phe_continue:
  StrLen $PHE_NeedleLen $PHE_Needle
  StrLen $PHE_HayLen $PHE_Haystack

  ; exact sole-entry match
  StrCmp $PHE_Haystack $PHE_Needle phe_found phe_check_leading

  phe_check_leading:
    IntOp $PHE_CompoundLen $PHE_NeedleLen + 1
    StrCpy $PHE_Slice $PHE_Haystack $PHE_CompoundLen
    StrCpy $PHE_Compound "$PHE_Needle;"
    StrCmp $PHE_Slice $PHE_Compound phe_found phe_check_mid

  phe_check_mid:
    StrCpy $PHE_Compound ";$PHE_Needle;"
    Push $PHE_Haystack
    Push $PHE_Compound
    Call NotemdStrIndexOf
    Pop $PHE_Idx
    StrCmp $PHE_Idx "" phe_check_trailing phe_found

  phe_check_trailing:
    StrCpy $PHE_Compound ";$PHE_Needle"
    StrLen $PHE_CompoundLen $PHE_Compound
    IntOp $PHE_Offset $PHE_HayLen - $PHE_CompoundLen
    StrCpy $PHE_Slice $PHE_Haystack $PHE_CompoundLen $PHE_Offset
    StrCmp $PHE_Slice $PHE_Compound phe_found phe_not_found

  phe_found:
    StrCpy $PHE_Result "1"
    Goto phe_done

  phe_not_found:
    StrCpy $PHE_Result ""

  phe_done:
  Pop $PHE_Needle
  Exch $PHE_Result
FunctionEnd

; ---------------------------------------------------------------------------
; un.NotemdRemoveFromPath — the uninstall-side mirror of the above. Expects
; $NotemdBin already set to "$INSTDIR\bin". Handles the same four anchored
; shapes and writes the result back to the registry; if the entry is not
; found at all (already removed, or PATH was hand-edited), the registry
; value is left untouched.
; ---------------------------------------------------------------------------
Function un.NotemdRemoveFromPath
  ; A read we cannot trust must never become a write. If the value exists but
  ; came back empty/truncated, splicing from "" and writing the result back
  ; would erase the user's PATH on uninstall — the same hazard as on install,
  ; with no second chance to notice. Leaving one stale entry behind (pointing
  ; at a directory that no longer exists, which Windows simply skips) is the
  ; strictly better failure.
  ClearErrors
  ReadRegStr $NotemdPath HKCU "Environment" "Path"
  IfErrors notemd_rm_read_failed notemd_rm_read_ok

  notemd_rm_read_failed:
    Call un.NotemdPathValueExists
    StrCmp $NotemdPathValueFound "1" notemd_rm_unreadable notemd_rm_done

  notemd_rm_unreadable:
    DetailPrint "notemd: could not read the user PATH; leaving it untouched."
    Goto notemd_rm_done

  notemd_rm_read_ok:
  StrCmp $NotemdPath "" notemd_rm_done

  ; Case: our bin dir is the entire PATH value.
  StrCmp $NotemdPath $NotemdBin notemd_rm_only notemd_rm_check_leading

  notemd_rm_only:
    StrCpy $NotemdPath ""
    Goto notemd_rm_write

  notemd_rm_check_leading:
    ; "$INSTDIR\bin;..." at the very start — reachable in completely
    ; ordinary use, not just via manual reordering: if PATH was empty at
    ; install time we write it bare, and any *other* installer appending
    ; later produces exactly this shape with nobody reordering anything.
    StrLen $NotemdNeedleLen $NotemdBin
    IntOp $NotemdTailStart $NotemdNeedleLen + 1
    StrCpy $NotemdCheckSlice $NotemdPath $NotemdTailStart
    StrCpy $NotemdNeedle "$NotemdBin;"
    StrCmp $NotemdCheckSlice $NotemdNeedle notemd_rm_do_leading notemd_rm_check_mid

  notemd_rm_do_leading:
    StrCpy $NotemdPath $NotemdPath "" $NotemdTailStart
    Goto notemd_rm_write

  notemd_rm_check_mid:
    ; "...;$INSTDIR\bin;..." — anchored on BOTH sides by ";", so a
    ; neighboring entry that merely starts with the same characters (e.g.
    ; "...\bin2") can never be mistaken for a match. This needle already
    ; carries both boundaries as literal characters, so trusting its FIRST
    ; occurrence is safe regardless of what else is in the string — unlike
    ; the review-round-1 bug, there is no way for a substring collision to
    ; satisfy this comparison.
    StrCpy $NotemdNeedle ";$NotemdBin;"
    Push $NotemdPath
    Push $NotemdNeedle
    Call un.NotemdStrIndexOf
    Pop $NotemdIdx
    StrCmp $NotemdIdx "" notemd_rm_check_trailing notemd_rm_do_mid

  notemd_rm_do_mid:
    ; Remove only ";$INSTDIR\bin" (not the trailing ";"), so the separator
    ; for whatever follows survives.
    StrCpy $NotemdNeedle ";$NotemdBin"
    StrLen $NotemdNeedleLen $NotemdNeedle
    StrCpy $NotemdLeft $NotemdPath $NotemdIdx
    IntOp $NotemdTailStart $NotemdIdx + $NotemdNeedleLen
    StrCpy $NotemdRight $NotemdPath "" $NotemdTailStart
    StrCpy $NotemdPath "$NotemdLeft$NotemdRight"
    Goto notemd_rm_write

  notemd_rm_check_trailing:
    ; "...;$INSTDIR\bin" at the very end — anchored on the left by ";" and
    ; on the right by end-of-string. This is a direct suffix comparison, not
    ; a search: the string either ends with this exact substring or it does
    ; not, so there is no "which occurrence" ambiguity the way a plain
    ; index search would have.
    StrCpy $NotemdNeedle ";$NotemdBin"
    StrLen $NotemdNeedleLen $NotemdNeedle
    StrLen $NotemdHayLen $NotemdPath
    IntOp $NotemdOffset $NotemdHayLen - $NotemdNeedleLen
    StrCpy $NotemdCheckSlice $NotemdPath $NotemdNeedleLen $NotemdOffset
    StrCmp $NotemdCheckSlice $NotemdNeedle notemd_rm_do_trailing notemd_rm_done

  notemd_rm_do_trailing:
    StrCpy $NotemdPath $NotemdPath $NotemdOffset
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
  ; by one entry on every single update until it breaks. The presence check
  ; is boundary-anchored (see NotemdPathHasEntry) so a neighboring entry
  ; like "...\bin2" is never mistaken for ours and does not cause us to
  ; silently skip the append (review round 1, Important finding #2).
  StrCpy $NotemdBin "$INSTDIR\bin"
  ; An unreadable PATH must not be mistaken for an absent one — see the
  ; READ FAILURES header note. On an unreadable-but-present value we write
  ; nothing at all: the user loses the `notemd` command on PATH (recoverable
  ; in ten seconds by hand) instead of losing their PATH (not recoverable).
  ClearErrors
  ReadRegStr $NotemdPath HKCU "Environment" "Path"
  IfErrors notemd_path_read_failed notemd_path_read_ok

  notemd_path_read_failed:
    Call NotemdPathValueExists
    StrCmp $NotemdPathValueFound "1" notemd_path_unreadable notemd_path_absent

  notemd_path_unreadable:
    DetailPrint "notemd: could not read the user PATH; leaving it untouched. Add $INSTDIR\bin to it by hand to use the notemd command."
    Goto notemd_path_done

  notemd_path_absent:
    ; No Path value under HKCU\Environment at all: creating one is safe, and
    ; is the ordinary case on a machine that has never had a per-user PATH.
    StrCpy $NotemdPath ""

  notemd_path_read_ok:
  Push $NotemdPath
  Push $NotemdBin
  Call NotemdPathHasEntry
  Pop $NotemdHasEntry
  StrCmp $NotemdHasEntry "1" notemd_path_done notemd_path_check_empty

  notemd_path_check_empty:
    StrCmp $NotemdPath "" notemd_path_set_bare notemd_path_check_sep

    notemd_path_check_sep:
      ; Avoid producing "...;;bin" if the existing value already ends with a
      ; separator (several installers leave one behind). An empty PATH
      ; segment historically means "search the current directory" in
      ; classic Win32 resolution — not a side effect an installer should
      ; introduce (review round 1, Important finding #3). $NotemdPath is
      ; non-empty on this branch, so HayLen - 1 cannot go negative.
      StrLen $NotemdHayLen $NotemdPath
      IntOp $NotemdOffset $NotemdHayLen - 1
      StrCpy $NotemdCheckSlice $NotemdPath 1 $NotemdOffset
      StrCmp $NotemdCheckSlice ";" notemd_path_append_no_sep notemd_path_append_with_sep

    notemd_path_append_no_sep:
      StrCpy $NotemdPath "$NotemdPath$NotemdBin"
      Goto notemd_path_write

    notemd_path_append_with_sep:
      StrCpy $NotemdPath "$NotemdPath;$NotemdBin"
      Goto notemd_path_write

    notemd_path_set_bare:
      StrCpy $NotemdPath "$NotemdBin"
      Goto notemd_path_write

  notemd_path_write:
    WriteRegExpandStr HKCU "Environment" "Path" "$NotemdPath"
    SendMessage 0xFFFF 0x1A 0 "STR:Environment" /TIMEOUT=5000

  notemd_path_done:
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ; File removal stays in PRE because it must precede Tauri's own
  ; non-recursive `RMDir "$INSTDIR"` — bin\ has to be gone by then or
  ; $INSTDIR survives as a stray directory.
  Delete "$INSTDIR\bin\notemd.cmd"
  ; No /r: this only succeeds if bin\ is empty. If a user dropped their own
  ; files in there, that is not ours to delete, and we leave the directory.
  RMDir "$INSTDIR\bin"
!macroend

; The PATH edit runs POST, not PRE, because Tauri's uninstaller shows a
; cancellable "the app is still running, close it?" prompt *after* the
; PREUNINSTALL hook. Editing PATH there meant that cancelling an uninstall
; left a fully installed app whose `notemd` command had silently vanished
; from every shell — a destructive side effect of an operation the user
; explicitly aborted. POSTUNINSTALL only runs once the uninstall has actually
; gone through.
!macro NSIS_HOOK_POSTUNINSTALL
  StrCpy $NotemdBin "$INSTDIR\bin"
  Call un.NotemdRemoveFromPath
!macroend
