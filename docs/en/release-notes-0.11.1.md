# Beetle Memory 0.11.1 Source Release Notes

0.11.1 fixes the long-term memory catalog and detail capacity gap in 0.11.0. The `v0.11.1` source tag and its commit identify this release.

The catalog now scans bounded batches within one immutable Store read session. Subject visibility, source withdrawal, and terminal-state filters run before compact catalog rows are used for exact totals and pagination. Detail reads only the requested owner's closure, so unrelated records cannot exhaust the historical recall candidate budget. Cursors still bind the query, subject, and matching catalog. A matching change invalidates an old cursor; an unrelated change does not.

Public APIs, Store v16, long-term material v6, control revision v4, and Adapter V2 are unchanged. The [0.11.0 control integrity contract](release-notes-0.11.0.md) still applies. There is no automatic migration for older Stores. Use the same source tag for every Beetle crate.

The production macOS profile was checked with 43 facts across 20/20/3 pages, exact totals, detail, subject isolation, terminal filtering, and cursor behavior. File and SQLite were also checked after reopen. Related Core, SDK, and Store targeted regressions, formatting, and static checks passed. Windows/Linux/ESP integration, real data, Provider, and GUI remain unverified. This is a Git source and tag release only; no crates.io upload, hosted Release, installer, or deployment was made.
