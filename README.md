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
