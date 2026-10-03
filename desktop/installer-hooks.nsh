; Aim View's installer hooks (tauri.conf.json: bundle.windows.nsis.installerHooks). They put the DLLs the review needs
; beside the app's exe, where Windows looks first, so the app runs on a computer with nothing else installed:
; - DirectML.dll, the version ONNX Runtime was built for (Windows has an older one of its own). The ort crate's build
;   puts it beside the exe in target/<profile>/.
; - the VC++ runtime ONNX Runtime needs (msvcp140 and the DLLs it loads). build.rs copies it from Visual Studio into
;   target/<profile>/vc-runtime/.
; ONNX Runtime itself is linked into the exe.
; MAINBINARYSRCPATH is the exe in target/<profile>/, so "<exe>\.." is that folder.

!macro NSIS_HOOK_POSTINSTALL
  SetOutPath "$INSTDIR"
  File "${MAINBINARYSRCPATH}\..\DirectML.dll"
  File "${MAINBINARYSRCPATH}\..\vc-runtime\msvcp140.dll"
  File "${MAINBINARYSRCPATH}\..\vc-runtime\msvcp140_1.dll"
  File "${MAINBINARYSRCPATH}\..\vc-runtime\vcruntime140.dll"
  File "${MAINBINARYSRCPATH}\..\vc-runtime\vcruntime140_1.dll"
!macroend

; the uninstaller has already removed the app's own files and tried to remove its folder
!macro NSIS_HOOK_POSTUNINSTALL
  Delete "$INSTDIR\DirectML.dll"
  Delete "$INSTDIR\msvcp140.dll"
  Delete "$INSTDIR\msvcp140_1.dll"
  Delete "$INSTDIR\vcruntime140.dll"
  Delete "$INSTDIR\vcruntime140_1.dll"
  RMDir "$INSTDIR"
!macroend
