# claude-meter

A lightweight Rust CLI that acts as a local OTLP HTTP/protobuf collector for Claude Code telemetry. It receives metrics from Claude Code, stores them in SQLite, and provides CLI commands to query usage data — tokens, costs, sessions, and more.

## Install

```bash
cargo install --path .
```

## Quick Start

### 1. Start the collector

```bash
claude-meter serve
```

Listens on `localhost:4318` by default. Data is stored in `~/.claude-meter/metrics.db`.

### 2. Run Claude Code with telemetry

```bash
export CLAUDE_CODE_ENABLE_TELEMETRY=1
export OTEL_METRICS_EXPORTER=otlp
export OTEL_EXPORTER_OTLP_PROTOCOL=http/protobuf
export OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318
export OTEL_METRIC_EXPORT_INTERVAL=10000
claude
```

You can add these exports to your shell profile (`~/.zshrc`, `~/.bashrc`) so they persist across sessions.

### 3. Query your data

```bash
claude-meter today         # today's usage
claude-meter totals        # all-time totals
claude-meter history       # daily breakdown (last 7 days)
claude-meter by-model      # usage per model
claude-meter by-session    # usage per session
claude-meter --version     # show version
```

## Commands

### `claude-meter serve`

Start the OTLP collector server.

```
claude-meter serve [--port 4318] [--db ~/.claude-meter/metrics.db]
```

### `claude-meter today`

Today's usage summary.

```
$ claude-meter today

Today
────────────────────────────────────────
Tokens (input)                   65,550
  input                          12,450
  cache read                     45,000
  cache creation                  8,100
Tokens (output)                   3,200
Cost                              $0.42
Sessions                              1
Lines added                         156
Lines removed                        34
Active time                     23m 15s
```

### `claude-meter totals`

All-time totals with earliest data date.

```
$ claude-meter totals

All Time (since Sat 22 Mar 2026)
────────────────────────────────────────
Tokens (input)                3,823,111
  input                             492
  cache read                  3,558,147
  cache creation                264,472
Tokens (output)                  28,541
Cost                              $3.62
Sessions                             10
Lines added                          96
Lines removed                         7
Active time                     17m 57s
```

### `claude-meter history`

Daily breakdown table.

```
$ claude-meter history --days 7

┌────────────┬────────┬────────┬────────────┬───────┬──────────┐
│ Date       ┆ Input  ┆ Output ┆ Cache Read ┆ Cost  ┆ Sessions │
╞════════════╪════════╪════════╪════════════╪═══════╪══════════╡
│ 2026-03-23 ┆ 12,450 ┆ 3,200  ┆ 45,000     ┆ $0.42 ┆ 1        │
├╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌┤
│ 2026-03-22 ┆ 79,100 ┆ 20,900 ┆ 120,000    ┆ $2.35 ┆ 2        │
└────────────┴────────┴────────┴────────────┴───────┴──────────┘
```

### `claude-meter by-model`

Usage breakdown by model.

```
$ claude-meter by-model

┌───────────────────┬────────┬────────┬───────┐
│ Model             ┆ Input  ┆ Output ┆ Cost  │
╞═══════════════════╪════════╪════════╪═══════╡
│ claude-opus-4-6   ┆ 46,550 ┆ 12,100 ┆ $1.57 │
├╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌┤
│ claude-sonnet-4-6 ┆ 45,000 ┆ 12,000 ┆ $1.20 │
└───────────────────┴────────┴────────┴───────┘
```

### `claude-meter by-session`

Per-session usage summary.

```
$ claude-meter by-session --days 7

┌────────────┬────────────┬─────────┬───────┬──────────┐
│ Session    ┆ Date       ┆ Tokens  ┆ Cost  ┆ Duration │
╞════════════╪════════════╪═════════╪═══════╪══════════╡
│ sess-a3f.. ┆ 2026-03-23 ┆ 68,750  ┆ $0.42 ┆ 23m 15s  │
├╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌┤
│ sess-b7c.. ┆ 2026-03-22 ┆ 163,000 ┆ $1.15 ┆ 45m 0s   │
└────────────┴────────────┴─────────┴───────┴──────────┘
```

### `claude-meter purge`

Delete data older than a given duration. Asks for confirmation before deleting.

```
$ claude-meter purge --older-than 30d

This will delete 142 row(s) older than 30d.
Continue? [y/N] y
Deleted 142 row(s).
```

Supported duration formats: `30d` (days), `6m` (months), `1y` (years).

### `claude-meter --version`

Show the installed version.

```
$ claude-meter --version
claude-meter 0.1.0
```

## Global Options

All query commands accept `--db <PATH>` to use a custom database location. Default: `~/.claude-meter/metrics.db`.

## How It Works

1. Claude Code exports OpenTelemetry metrics via OTLP HTTP/protobuf
2. `claude-meter serve` receives `POST /v1/metrics` requests, decodes the protobuf payload, and inserts metrics into SQLite
3. Cumulative counters are automatically converted to deltas to avoid double-counting
4. Query commands read directly from the SQLite database — the server doesn't need to be running to query

## Project Structure

```
src/
├── main.rs      # CLI entry point, clap subcommands
├── lib.rs       # Public module exports
├── server.rs    # Axum HTTP server, OTLP protobuf parsing
├── db.rs        # SQLite schema, insert, query functions
└── display.rs   # Terminal table formatting
tests/
└── e2e.rs       # End-to-end integration test
```

## Building from Source

```bash
git clone https://github.com/juliooa/claude-meter.git
cd claude-meter
cargo build --release
```

The binary will be at `target/release/claude-meter`.

## Running Tests

```bash
cargo test
```

## License

MIT
