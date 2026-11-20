; Kernest installer hooks: Explorer integration.
;
; - "Open in Kernest" on the right-click menu of .kernest fonts, which are folders,
;   and of .ufo and .glyphspackage sources (also folders).
; - "Export fonts here" and "Check font" on the same menus: the command line builds
;   the fonts into a build folder next to the source (and checks them).
; - .glyphs and .designspace files get "Open with Kernest". Kernest only becomes
;   their default app if no other app has claimed them.
;
; SHCTX is HKCU for a per-user install and HKLM for an all-users install.

!define KERNEST_EXE "$INSTDIR\${MAINBINARYNAME}.exe"
!define KERNEST_FOLDER_VERB "Software\Classes\Directory\shell\Kernest.Open"
!define KERNEST_PROGID "Kernest.Source"
!define KERNEST_CLI "$INSTDIR\kernest-cli.exe"
!define KERNEST_EXPORT_VERB "Software\Classes\Directory\shell\Kernest.Export"
!define KERNEST_CHECK_VERB "Software\Classes\Directory\shell\Kernest.Check"
!define KERNEST_APPLIES 'System.FileName:"*.kernest" OR System.FileName:"*.ufo" OR System.FileName:"*.glyphspackage"'
; The command window stays open so the result can be read.
!define KERNEST_EXPORT_CMD 'cmd.exe /c ""${KERNEST_CLI}" export "%1" --out "%1\..\build" & echo. & pause"'
!define KERNEST_CHECK_CMD 'cmd.exe /c ""${KERNEST_CLI}" export "%1" --out "%1\..\build" && "${KERNEST_CLI}" check "%1\..\build" & echo. & pause"'

; The commands hold quotes, so they're used through their defines here: passed as
; macro parameters, NSIS would split them at the quotes.
!macro KERNEST_EXPORT_VERB_AT KEY
  WriteRegStr SHCTX "${KEY}" "" "Export fonts here"
  WriteRegStr SHCTX "${KEY}" "Icon" "${KERNEST_EXE},0"
  WriteRegStr SHCTX "${KEY}\command" "" '${KERNEST_EXPORT_CMD}'
!macroend

!macro KERNEST_CHECK_VERB_AT KEY
  WriteRegStr SHCTX "${KEY}" "" "Check font"
  WriteRegStr SHCTX "${KEY}" "Icon" "${KERNEST_EXE},0"
  WriteRegStr SHCTX "${KEY}\command" "" '${KERNEST_CHECK_CMD}'
!macroend

!macro KERNEST_CLAIM_EXT EXT
  WriteRegStr SHCTX "Software\Classes\${EXT}\OpenWithProgids" "${KERNEST_PROGID}" ""
  ReadRegStr $0 SHCTX "Software\Classes\${EXT}" ""
  StrCmp $0 "" 0 +2
    WriteRegStr SHCTX "Software\Classes\${EXT}" "" "${KERNEST_PROGID}"
!macroend

!macro KERNEST_RELEASE_EXT EXT
  DeleteRegValue SHCTX "Software\Classes\${EXT}\OpenWithProgids" "${KERNEST_PROGID}"
  ReadRegStr $0 SHCTX "Software\Classes\${EXT}" ""
  StrCmp $0 "${KERNEST_PROGID}" 0 +2
    DeleteRegValue SHCTX "Software\Classes\${EXT}" ""
  DeleteRegKey /ifempty SHCTX "Software\Classes\${EXT}\OpenWithProgids"
  DeleteRegKey /ifempty SHCTX "Software\Classes\${EXT}"
!macroend

!macro NSIS_HOOK_POSTINSTALL
  ; Right-click "Open in Kernest" on .kernest, .ufo and .glyphspackage folders.
  WriteRegStr SHCTX "${KERNEST_FOLDER_VERB}" "" "Open in Kernest"
  WriteRegStr SHCTX "${KERNEST_FOLDER_VERB}" "Icon" "${KERNEST_EXE},0"
  WriteRegStr SHCTX "${KERNEST_FOLDER_VERB}" "AppliesTo" 'System.FileName:"*.kernest" OR System.FileName:"*.ufo" OR System.FileName:"*.glyphspackage"'
  WriteRegStr SHCTX "${KERNEST_FOLDER_VERB}\command" "" '"${KERNEST_EXE}" "%1"'

  ; Right-click "Export fonts here" and "Check font" on the same folders.
  !insertmacro KERNEST_EXPORT_VERB_AT "${KERNEST_EXPORT_VERB}"
  WriteRegStr SHCTX "${KERNEST_EXPORT_VERB}" "AppliesTo" '${KERNEST_APPLIES}'
  !insertmacro KERNEST_CHECK_VERB_AT "${KERNEST_CHECK_VERB}"
  WriteRegStr SHCTX "${KERNEST_CHECK_VERB}" "AppliesTo" '${KERNEST_APPLIES}'

  ; Font sources that are files.
  WriteRegStr SHCTX "Software\Classes\${KERNEST_PROGID}" "" "Font source"
  WriteRegStr SHCTX "Software\Classes\${KERNEST_PROGID}\DefaultIcon" "" "${KERNEST_EXE},0"
  WriteRegStr SHCTX "Software\Classes\${KERNEST_PROGID}\shell\open" "" "Open with Kernest"
  WriteRegStr SHCTX "Software\Classes\${KERNEST_PROGID}\shell\open\command" "" '"${KERNEST_EXE}" "%1"'
  !insertmacro KERNEST_EXPORT_VERB_AT "Software\Classes\${KERNEST_PROGID}\shell\Kernest.Export"
  !insertmacro KERNEST_CHECK_VERB_AT "Software\Classes\${KERNEST_PROGID}\shell\Kernest.Check"
  !insertmacro KERNEST_CLAIM_EXT ".glyphs"
  !insertmacro KERNEST_CLAIM_EXT ".designspace"

  ; Tell Explorer that file associations changed.
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  DeleteRegKey SHCTX "${KERNEST_FOLDER_VERB}"
  DeleteRegKey SHCTX "${KERNEST_EXPORT_VERB}"
  DeleteRegKey SHCTX "${KERNEST_CHECK_VERB}"
  !insertmacro KERNEST_RELEASE_EXT ".glyphs"
  !insertmacro KERNEST_RELEASE_EXT ".designspace"
  DeleteRegKey SHCTX "Software\Classes\${KERNEST_PROGID}"
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
!macroend
