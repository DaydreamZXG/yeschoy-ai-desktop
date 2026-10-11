# station-connect-mock

Local **development-only** HTTP server that implements:

- `POST /api/desktop/v2/connect-codes` (session cookie)
- `POST /api/desktop/v2/connect-codes/exchange`
- Browser harness at `/` («打开桌面助手» flow)

See [`../../docs/station-connect-e2e-mock.md`](../../docs/station-connect-e2e-mock.md) and
[`../../docs/station-connect-api.openapi.yaml`](../../docs/station-connect-api.openapi.yaml).

```bash
cargo run
```
