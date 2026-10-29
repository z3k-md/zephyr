; Uninstalling removes the CPU temperature helper's task and its copy under Program Files
; (cputemp.rs), the way turning the reading off does: Zephyr itself runs the removal, after
; Windows asks for permission. Declining leaves them behind; the helper then quits on its
; own, since Zephyr is gone. Updates don't uninstall first, so they leave the helper be.
!macro NSIS_HOOK_PREUNINSTALL
  ${If} $UpdateMode <> 1
  ${AndIf} ${FileExists} "$PROGRAMFILES64\Zephyr CPU Temperature\*.*"
    ExecWait '"$INSTDIR\${MAINBINARYNAME}.exe" --cpu-temp-helper --remove'
  ${EndIf}
!macroend
