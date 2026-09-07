# RU-073 contract

This release unit prepares the client-owned OAuth 2.0 Authorization Code + PKCE S256 boundary before the server rollout. The module is compiled and unit-tested, but it is deliberately not connected to the current Tauri command surface.

- Bind and retain `127.0.0.1:0` before creating the redirect URI.
- Generate independent 32-byte `state` and verifier values and derive an S256 challenge.
- Accept only `https://yeschoy.com` or `https://ai.yeschoy.io` as the compiled browser authorization-page origin.
- Keep token, refresh, user-info and revoke authority fixed at `https://yeschoy.com`.
- Accept only one exact loopback GET callback with an exact Host, path, state and one code-or-error result.
- Reject malformed, foreign, duplicated, expired, cancelled and replayed callbacks without consuming a still-valid flow or exposing credential material.
- Build form-encoded code, refresh and revoke requests without a client secret or automatic credential replay.

The active account path remains `desktop-account-session@v5` and continues to use the deployed `/api/desktop/v2/*` device flow. This unit does not select billing groups, replace tool keys, write credentials, modify application adapters, expose a frontend command, package an installer or claim live server interoperability.

Activation belongs to a later release unit after the backend deploys a capability declaration and defines how a narrow OAuth bearer is used for account data, billing groups and all local application bridges.
