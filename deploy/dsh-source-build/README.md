# DeepSeek Harness Desktop source-build boundary

This directory prepares a future Yeschoy-distributed Desktop build from a single pinned official
DeepSeek Harness source revision. It deliberately does **not** publish anything and it never puts
GitHub on the end-user download path.

The artifact label is `Yeschoy-built from official DeepSeek source`. It must not be described as an
official DeepSeek release. The upstream MIT notice is retained in
`LICENSE.deepseek-harness`.

## Current release gate

`build-spec.json` pins the repository, commit, tree, package versions, application ID, license and
three native package commands. All `publisherId` values intentionally remain `null`. Consequently,
`verify_native.py` and `prepare_candidate.py` fail with `signing_policy_unconfigured`; no public
candidate can be produced accidentally.

Before a public build, review and commit exact publisher identities:

- `win-x64`: the uppercase 40-character Authenticode signer certificate thumbprint. Run only on a
  controlled Windows x64 signer with `DSH_DESKTOP_WINDOWS_SIGNTOOL` pointing to the validated
  SignTool executable.
- `mac-arm64` and `mac-x64`: the exact 10-character Apple Team ID. Run on the native macOS target
  after signing, notarization and stapling.

Changing the source revision, application ID, publisher or targets requires a newly reviewed build
policy. Do not edit an already used candidate identity.

## Private operator flow

1. Clone the official repository on a builder that can access GitHub and check out the pinned
   commit. End-user machines never perform this step.
2. Run the target command listed in `build-spec.json` using the upstream documented signing and
   notarization environment.
3. Produce the source receipt:

   ```text
   python3 deploy/dsh-source-build/verify_source.py \
     --source /private/path/deepseek-harness \
     --receipt /private/path/source-receipt.json
   ```

4. On the matching native host, verify the exact artifact and write a native receipt:

   ```text
   python3 deploy/dsh-source-build/verify_native.py \
     --target mac-arm64 \
     --artifact /private/path/DeepSeek-Harness.dmg \
     --source-receipt /private/path/source-receipt.json \
     --output /private/path/native-receipt.json
   ```

5. Prepare closed, public-safe candidate metadata:

   ```text
   python3 deploy/dsh-source-build/prepare_candidate.py \
     --artifact /private/path/DeepSeek-Harness.dmg \
     --source-receipt /private/path/source-receipt.json \
     --native-receipt /private/path/native-receipt.json \
     --output /private/path/candidate.json
   ```

The tools reject unknown JSON fields, changed source, dirty tracked files, wrong license/version,
symlink or hardlink inputs, wrong artifact bytes, receipt replay, wrong publisher, unsigned Windows
output and unstapled macOS output. Receipts contain no local checkout path or credential.

Candidate preparation still does not upload files, alter the existing official-byte mirror catalog,
or make a client feature available. Publication and client catalog wiring must be a later, separately
reviewed release unit after real native packages pass clean-device installation tests.
