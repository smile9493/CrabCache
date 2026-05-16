# Management API manual verification

1. Copy `config/gateway.example.toml` to `config/gateway.toml` and set a real `api_key`.
2. Start Redis: `docker compose up -d redis`
3. Run gateway: `cargo run -p crab-gateway -- config/gateway.toml`
4. Create a key:
   ```bash
   curl -s -X POST http://127.0.0.1:9080/v1/keys \
     -H 'X-Gateway-Admin-Key: dev-only-gateway-admin-secret' \
     -H 'Content-Type: application/json' \
     -d '{"name":"test","enabled":true}'
   ```
5. Use returned `key_full` against chat completions on `:8080`.
6. Start admin with `CRABCACHE_GATEWAY_CONTROL_URL=http://127.0.0.1:9080` and the same admin key; create/revoke keys from the dashboard.
