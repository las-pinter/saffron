# Saffron design guide

This guide describes the look and behavior of the Saffron web UI. It is the
reference for anyone, human or AI agent, building frontend code. The mockups in
[`mockups/`](mockups/) are static HTML pages with sample data. Open them in any
browser; they need no build step and no runtime.

## Status

- **Layout:** the "calm wealth app" direction is chosen: generous spacing,
  large clear numbers, details one click away.
- **Theme:** **saffron night** (dark) is the chosen look and the planned
  default.
- **Light theme:** a warm, dimmed light theme is planned but not designed yet.
  The first bright draft was rejected as too bright.
- **Rejected:** a dense, terminal-style dashboard.

## Mockups

| Page | File | Shows |
|------|------|-------|
| Overview | [`mockups/overview.html`](mockups/overview.html) | Net worth and chart vs benchmark, items that need attention, holdings table, allocation vs target, AI digest preview, tax card (post-v1), news |
| Holding detail | [`mockups/holding.html`](mockups/holding.html) | Price chart with buy markers, purchase lots (FIFO), position stats, AI view card, earlier AI calls with outcomes, news with impact, alert rules |
| AI desk | [`mockups/ai-desk.html`](mockups/ai-desk.html) | Daily digest, open AI calls, track record and confidence calibration, AI spend vs monthly cap, question box, connected MCP agents |
| Discover | [`mockups/discover.html`](mockups/discover.html) | Daily scan of the investable universe, universe filter chips, ranked candidates, detail panel with portfolio-fit preview |
| Phone overview | [`mockups/phone-overview.html`](mockups/phone-overview.html) | 390 × 844 phone layout: net worth, attention list, top holdings, bottom tab bar |

All names, numbers, news, and AI calls in the mockups are sample data for
layout only.

## Color tokens: saffron night

The palette takes its colors from the saffron plant: red-brown dried threads,
crimson stigmas, golden color, and the lilac crocus flower.

| Token | Hex | Use |
|-------|-----|-----|
| `bg` | `#1A0F0A` | Page background |
| `surface` | `#26160F` | Cards, header, table areas |
| `surface-sunken` | `#20130C` | Inputs, inset tiles |
| `border` | `#4A2C1D` | Card and control borders |
| `divider` | `#3A2217` | Table row lines, progress-bar tracks, chart grid |
| `text` | `#F6E7D3` | Primary text |
| `text-secondary` | `#E3CBAE` | Secondary text, table cells |
| `text-muted` | `#C4A585` | Labels, captions, metadata |
| `text-faint` | `#A88A6C` | Decorative only (e.g. cents after a big number); never for content that must be read |
| `accent` | `#F5B32E` | Saffron gold: primary buttons, links, active chips, chart line, progress fills |
| `accent-strong` | `#F5A020` | Chart area fill (at 22% → 0% opacity) |
| `accent-soft` | `#4D2410` | Active navigation background, logo disc |
| `accent-on-soft` | `#FFC861` | Text on `accent-soft` |
| `on-accent` | `#1A0F0A` | Text on `accent` |
| `crimson` | `#E8552B` | Brand mark (logo stigmas), notification badge |
| `ai-surface` | `#4A1A0C` | Background of AI opinion cards (digest, AI view) |
| `ai-track` | `#5E2E1A` | Progress tracks inside AI cards |
| `ai-text` | `#FBEBD6` | Text on `ai-surface` |
| `ai-text-muted` | `#D2B08C` / `#EBD3B5` | Metadata on `ai-surface` |
| `gain` | `#7BD3A6` | Positive change, always with ▲ |
| `loss` | `#FF8FA0` | Negative change, always with ▼ |
| `loss-soft` | `#40161F` | Background of loss/news alerts |
| `gain-soft` | `#163326` | Background of "Buy" tags |
| `warn-soft` | `#42260E` | Background of drift alerts and the selected table row |
| `crocus` | `#C3AEF5` | "Idea" and "Watch" tags, bond allocation bar |
| `crocus-soft` | `#2B1F3D` | Background of idea alerts and "Watch" tags |

Rules:

- Gains and losses always carry an arrow (▲ / ▼) or sign as well as color.
  Meaning never depends on color alone.
- Loss is rose, not red-orange, so it is never confused with the saffron accent
  or the crimson brand color.
- Text contrast is at least 4.5:1 against its background, including small
  labels. `text-faint` is decorative only.
- Define the tokens as CSS custom properties, so the light theme can redefine
  them later.

## Typography

- **UI text:** Inter, 14 px base, 400/500/600/700.
- **Headings and big numbers:** Inter Tight, 600, letter-spacing −0.02 em
  (−0.03 em at 40 px and above).
- **All numbers:** `font-variant-numeric: tabular-nums`, so columns align.
- **Scale:** page title 30–32 px; net worth 48 px (phone 40 px); card title
  18 px; table text 14 px; captions 12 px.

## Number, date, and money format

- Base currency EUR. Amounts use the Finnish/European format: `€84 210,40`
  (narrow space for thousands, comma for decimals). Format through `Intl`
  with the user's locale; do not hand-build strings.
- Percentages: `+1,22 %`. Percentage-point differences: `−1,3 pp`.
- Every price and every computed figure shown with an AI opinion carries an
  "as of" time (e.g. "prices as of 17:35 EET", "NAV 23 Nov" for funds).

## Layout

- Max content width 1320 px, 32 px page padding, 24 px gap between cards.
- Two-column body: main column (`flex: 999 1 640px`) and side column
  (`flex: 1 1 360px`). On narrow screens the side column wraps below.
- Cards: `surface` background, 1 px `border`, 16 px radius, 24 px padding
  (28 px for hero cards).
- Wide tables scroll horizontally inside their card; the page never scrolls
  sideways.
- Phone: single column, 20 px side padding, bottom tab bar with four tabs
  (Overview, Holdings, Discover, AI desk); touch targets at least 44 px.

## Navigation

Top bar: logo, then Overview, Holdings, Discover, AI desk (Taxes is post-v1); search
(ticker, ISIN, or fund name), alerts bell with unread count, account menu.
The active item uses `accent-soft` background with `accent-on-soft` text.

## Components

- **Attention item:** a tag (News / Drift / Idea), a one-line title, one or two
  lines of explanation including the AI view where relevant, and one action
  link ("Review", "Plan rebalance", "See why"). Background tinted by type
  (`loss-soft`, `warn-soft`, `crocus-soft`).
- **AI view card:** `ai-surface` background. Shows the call (Buy / Hold / Sell /
  Watch) large, a confidence bar, three short reasons, what would change the
  call, a collapsible "Data the AI used" section, and the line "Opinion, not
  financial advice."
- **AI call tag:** Buy (`gain-soft` / `gain`), Hold (`divider` / `text`), Sell
  (`loss-soft` / `loss`), Watch (`crocus-soft` / `crocus`).
- **Track-record item:** call, confidence, date, and the outcome after 1, 3, or
  12 months, marked ✓ right, ✗ wrong, or ≈ neutral.
- **Allocation bar:** a filled bar for the actual share with a thin vertical
  marker at the target share, and "actual / target" text.
- **Chart:** portfolio line in `accent` over a fading `accent-strong` area;
  benchmark as a dashed `text-muted` line; deposits as a stepped `divider` area.
  Every chart has a text alternative (`role="img"` with `aria-label`).
- **Range selector:** 1M, 3M, YTD, 1Y, All, as toggle buttons with
  `aria-pressed`.
- **Filter chips:** pill buttons; the active chip uses `accent` with `on-accent`
  text.
- **News item:** headline link, an AI summary with the impact on the user's
  position, then source, time, and sentiment (▲ Positive / ▼ Negative /
  ● Neutral).

## Content rules for AI output in the UI

- Every AI opinion is labeled as an opinion, not financial advice.
- Numbers in AI text are computed by Saffron and passed to the model; the UI
  shows which data the AI used and its "as of" time.
- Saffron never places orders. Actions are "record a transaction", "add to
  watchlist", "add alert rule", or "ask a follow-up".

## Accessibility

- Use real `<button>`, `<a href>`, `<input>` with `<label>`, and `<table>` with
  `<th scope>`. Icon-only buttons have `aria-label`.
- Decorative SVG has `aria-hidden="true"`.
- Visible focus outlines on all interactive elements.
- Respect `prefers-reduced-motion`.
