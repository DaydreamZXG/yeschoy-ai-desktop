# RU-072 contract

The only product variation in this release unit is the external browser page used for device-login confirmation.

- Default build: `https://yeschoy.com/desktop-authorize`
- Friend build: `https://ai.yeschoy.io/desktop-authorize`
- Build selector: `YESCHOY_AUTHORIZATION_PAGE_ORIGIN`
- Allowed selector values: the two exact HTTPS origins above

The native account module must validate the server-returned canonical Yeschoy device-authorization URL and validated `user_code` first. It then constructs a new browser URL locally from the compiled origin, fixed path, and one `user_code` query parameter. It must not copy a server-returned host, path, query, fragment, credentials, Token endpoint or API endpoint into that browser target.

This unit preserves `desktop-account-session@v5`. It does not activate the proposed OAuth PKCE endpoints, change session storage, request a new privilege, modify a tool credential, package an installer, publish an update, or mutate any remote system.
