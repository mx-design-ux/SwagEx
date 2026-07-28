!macro NSIS_HOOK_POSTINSTALL
  ; Remove the obsolete per-user installer metadata left by versions before
  ; 0.3.0. The product data itself is migrated by SwagEx at first launch.
  DeleteRegKey HKCU "Software\jmlebret\SwagEx"
  DeleteRegKey /ifempty HKCU "Software\jmlebret"
!macroend
