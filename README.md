# Saffron

Saffron is a self-hosted portfolio monitor for individuals and households. It
records savings, stocks, ETFs, and funds with their purchase lots, tracks
prices, computes performance, watches the news, and tells you what needs
attention. An optional AI layer gives grounded opinions on your holdings and
keeps a track record of how those opinions turned out. A read-only MCP
endpoint lets AI agents query your data.

The first data-coverage target is what a Finnish investor can buy through a
local bank (Finnish funds, Nasdaq Helsinki stocks, UCITS ETFs, US stocks), with
EUR as the base currency. Nothing country-specific is hard-coded: currencies,
exchanges, and data providers are configuration.

> **Status:** early development. Nothing is usable yet.

## Planned features (v1)

- Accounts, transactions, and purchase lots (FIFO), cash and savings accounts
- Prices and FX rates with freshness tracking; manual entry as a fallback
- Performance: time-weighted return, XIRR, benchmark comparison, allocation
  drift against your targets
- Alerts in the app and as phone push notifications through ntfy
- News monitoring for your holdings
- AI analysis with any supported provider (Anthropic, OpenAI-compatible,
  Ollama): daily digest, buy/hold/sell/watch opinions with a measured track
  record, and an ask box; optional web search, off by default
- Daily discovery scan of a curated, editable set of instruments
- Read-only MCP server for AI agents
- A few user accounts with private or shared portfolios; passkey login
- One Docker container with SQLite, meant to run behind a VPN such as Tailscale

Saffron never places orders and is not financial advice.

## Design

The UI design guide and static mockups are in [`docs/design/`](docs/design/).

## Running with Docker

```sh
docker compose up -d   # builds the image from this checkout; data goes to the saffron-data volume
```

Saffron listens on `127.0.0.1:8640` on the host. Reach it through Tailscale
(`tailscale serve`) or a reverse proxy with HTTPS; do not expose it to the
internet. The container runs as a non-root user on a read-only root
filesystem, and `saffron healthcheck` is its built-in health probe.

On the first start, the log prints a one-time link that creates the
administrator account:

```sh
docker compose logs saffron | grep setup
```

The setup page itself arrives with the web interface. Until then, create the
administrator with the token from that link:

```sh
curl -X POST http://localhost:8640/api/auth/setup \
  -H 'Origin: http://localhost:8640' -H 'Content-Type: application/json' \
  -d '{"token":"<token>","username":"admin","display_name":"Admin","password":"<at least 12 characters>"}'
```

Settings, all optional, as environment variables (see `compose.yaml`):

| Variable | Default | Purpose |
|---|---|---|
| `SAFFRON_PUBLIC_URL` | `http://localhost:8640` | The HTTPS URL the browser uses. Required for any access other than localhost: cookies, the cross-site request check, and passkeys are bound to it. |
| `SAFFRON_TRUSTED_PROXIES` | none | Comma-separated addresses or CIDR ranges of proxies whose `X-Forwarded-For` header names the client, for per-client sign-in throttling. |
| `SAFFRON_BIND` | `127.0.0.1:8640` (`0.0.0.0:8640` in the image) | Listen address. |
| `SAFFRON_DATA_DIR` | `data` (`/data` in the image) | Database directory. |

## Development

Requirements: Rust (the version in `rust-toolchain.toml`), Node 22 with pnpm,
[just](https://github.com/casey/just), cargo-nextest, and cargo-deny.

```sh
just setup     # install web dependencies and git hooks
just run       # API server on http://127.0.0.1:8640
just dev-web   # web dev server, proxies /api to the API server
just gate      # the full quality gate: format, types, lint, tests, deny, build
```

Work happens on `dev`; `main` receives changes only through pull requests from
`dev`. Commit subjects follow Conventional Commits.

## License

[MIT](LICENSE)
