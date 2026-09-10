#!/bin/sh
# Reused RU-056 check-only resource shim. NEVER use for build/package/link.
# Windows Rust typechecking does not link resources. This check emits no code.
case " $* " in
  *" /? "*) printf '%s\n' 'OVERVIEW: LLVM Resource Converter'; exit 0 ;;
esac
output_path=""
while [ "$#" -gt 0 ]; do
  if [ "$1" = "/fo" ]; then shift; output_path="$1"; break; fi
  shift
done
if [ -n "$output_path" ]; then : > "$output_path"; fi
