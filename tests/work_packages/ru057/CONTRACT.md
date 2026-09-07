# RU-057 dual-platform internal packaging

Package the already verified RU-056 client as version 0.4.10. Only version metadata, release receipts and ignored local artifacts may change. The macOS DMG must be universal, Developer-ID signed, notarized, stapled and Gatekeeper accepted. The Windows x64 artifact must come from the existing Windows runner after its complete gates and remain explicitly unsigned internal software with Simplified-Chinese NSIS UI.

No runtime source, server, database, public download, GitHub Release, tag, updater artifact or update manifest change is authorized. Real-device installation and real-account use remain owner smoke tests after packaging.
