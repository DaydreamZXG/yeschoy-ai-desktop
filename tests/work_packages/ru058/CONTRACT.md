# RU-058 local cross-platform packaging

Preserve the exact 0.4.10 RU-056 runtime. The existing hosted Windows workflow failed before executing because of repository billing, so Windows may be built locally from exact commit `ffcfe898f79ff5069d7c4651a1bf0a779aa1a753` with locked `cargo-xwin` for `x86_64-pc-windows-msvc` and packaged by the checked-in closed NSIS recipe using the SHA-256 pinned Homebrew `makensis` 3.12 bottle from temporary storage.

The Windows artifact is unsigned, cross-built, requires an existing WebView2 Runtime and is not real-device verified. The macOS DMG must be universal, Developer-ID signed, notarized, stapled and Gatekeeper accepted. No runtime source, system-wide tool installation, public release, tag, updater, server or database change is permitted.
