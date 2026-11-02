# Preset sharing service — design sketch

A public library where G-System owners upload presets and banks, browse and
search them, and load them into the editor in one click. It replaces the
"TC Preset Vault" the Vyzor editor used to link to, which no longer exists.

Status: sketch for discussion. Nothing built yet.

## Goals

- Find a sound quickly: search by name, tags, effect types, what's switched on.
- See exactly what a preset does **before** loading it: the same decoded view
  the app shows.
- One click from the website or the app's Browse tab into an open bank, then
  Audition on the unit.
- Song banks: share a set of 5 presets as a unit, the way the G-System is played.
- Trustworthy files: every upload is validated; nothing invalid reaches a unit.

## Non-goals (for now)

- Hosting TC's factory presets (TC's content; everyone already has them).
- Comments/forums. Link out to existing communities instead.
- Paid anything.

## Architecture

```
            ┌──────────────── gsystem-protocol (Rust crate) ────────────────┐
            │ parse/validate dumps · decode params · view model · fingerprint │
            └───────┬───────────────────────┬────────────────────────┬──────┘
                    │                       │                        │
          desktop app (Tauri)        server (Actix + SQLite)    web preview (WASM, later)
          Browse tab, deep links     REST API + HTML pages      audition in Chrome via Web MIDI
```

One crate decodes presets everywhere, so the website, the API and the app
always agree on what a preset contains.

- **Server:** Rust, Actix Web (or Axum), `sqlx` + **SQLite** with FTS5 for
  search, Litestream for continuous backup to object storage. Presets are
  516 bytes; the whole library fits in a small database for a long time.
- **Location:** a `server/` crate in this repo, so it builds against the
  same `gsystem-protocol` revision as the app. It can move out later.
- **Hosting:** one small VM or Fly.io machine + an S3-compatible bucket for
  Litestream. A few dollars a month at this scale. A CDN in front of
  `/presets/*.syx` and the HTML pages.
- **Auth:** "Sign in with GitHub" and email magic links. Browsing and
  downloading need no account; uploading, voting and collections do.

## Data model

```sql
users(
  id, handle UNIQUE, display_name, github_id, email,
  created_at, banned_at
)

presets(
  id,                 -- short public id, e.g. "p_7Kx2"
  owner_id → users,
  name,               -- from the dump (20 chars), editable title separate
  title, description, -- markdown, length-limited
  license,            -- 'CC0-1.0' | 'CC-BY-4.0'
  data BLOB(516),     -- the preset payload, normalized
  fingerprint,        -- sha256 of the normalized payload (dedupe)
  firmware,           -- as declared by the uploader, e.g. '4.02'
  -- derived on upload by gsystem-protocol, stored for search/filter:
  blocks_on,          -- e.g. 'comp,delay,reverb'
  types,              -- e.g. 'Sustaining,Single Delay,Plate'
  tempo_bpm,
  uses_loops, uses_insert, uses_relays,  -- needs the uploader's external gear
  uses_expression, uses_novadrive,
  created_at, updated_at, downloads, score, hidden_at
)

tags(preset_id, tag)                         -- free-form, lowercased, max 8 per preset
collections(id, owner_id, title, description, kind)   -- kind: 'bank' (exactly 5) | 'list'
collection_items(collection_id, position, preset_id)
votes(user_id, preset_id, value)             -- +1 only, for "popular"
reports(id, preset_id, user_id, reason, created_at, resolved_at)
presets_fts(title, name, description, tags, types)    -- FTS5
```

**Normalization** before storing and fingerprinting:

- clear the name bytes after its NUL;
- mask the unexplained bit-7 flags in the name (the unit sets them);
- verify the checksum on the way in;
- check every field the parameter tables know is within range, and reject the upload if not.

Two uploads of the same sound then dedupe to one fingerprint, and
the second uploader is pointed at the existing preset.

## API (v1)

```
GET  /api/v1/presets?q=&tag=&type=&on=delay,reverb&no=loops&sort=new|popular&page=
GET  /api/v1/presets/{id}                 → metadata + decoded view (same JSON as the app's PresetView)
GET  /api/v1/presets/{id}.syx             → one 0x20 dump, numbered for user slot 0
POST /api/v1/presets                      ← .syx (one dump) or .Prs; title, tags, license
PATCH/DELETE /api/v1/presets/{id}         (owner)
GET  /api/v1/collections/{id}  ·  {id}.syx (bank: 5 dumps, slots 0–4)
POST /api/v1/collections
POST /api/v1/presets/{id}/vote · /report
GET  /api/v1/me · /auth/github · /auth/email
```

- Uploads: max 64 KB per request, rate-limited per account and IP.
- `.SQS` and multi-preset `.syx` imports can create several presets in one go,
  each still validated individually.
- Responses are cacheable, with ETags; the app keeps a small offline cache.

## Desktop app integration

- **Browse tab** (a 4th mode next to Banks / Preset / Global): search box,
  filters (effect types, "no loops", "uses expression pedal"), list on the
  left, decoded preset on the right using the existing Preset screen,
  read-only.
- **Load into slot…**: choose a slot in the open bank. Marks it edited, like
  any other change.
- **Audition**: uses the existing Live/edit-buffer path, so you hear it before
  loading anything into a bank. Pressing STORE on the unit keeps it.
- **Share**: from the inspector, "Upload this preset…" with title, tags and
  license prefilled from the dump; requires sign-in through the browser.
- **Deep links**: `gsystem://preset/{id}` and `gsystem://collection/{id}`
  (Tauri deep-link plugin). The website's "Open in G-System Editor" button
  uses them.
- **Badges** on every preset where they matter: *uses Loop 2*, *uses Insert*,
  *Expression pedal*, *NovaDrive*. These depend on the uploader's gear and
  will sound different on yours.

## Website

- Server-rendered pages (fast, indexable, linkable from forums):
  - home: new, popular, tags;
  - preset page: signal chain, every block's parameters, badges, download
    `.syx`, "Open in app";
  - collection page;
  - user page.
- Later: compile `gsystem-protocol` to WASM and use Web MIDI, so Chrome users
  can audition presets on their unit straight from the page, with no install.

## Trust and safety

- **Validation:** every dump passes checksum + range checks through the same
  crate as the app. Rejected uploads say why.
- **Licensing:** the uploader picks CC0 or CC-BY 4.0 and confirms the preset is
  theirs to share. No factory-preset uploads: detect them by fingerprint
  against the factory set and refuse.
- **Moderation:** a report button and hidden-pending-review. Names and descriptions
  are plain text or limited markdown, escaped on output.
- **Privacy:** store only what sign-in needs; public profile is handle +
  display name.

## Relationship with davidavalanches/gsystem-editor

Offer them the read API from day one: one shared community library is worth
more than two. The API returns raw `.syx`, which their app already imports.
Agree on the JSON shape early if they want decoded views too.

## Phases

1. **MVP:** server crate, GitHub sign-in, upload/validate/dedupe, search,
   preset pages, `.syx` download; app Browse tab with Load into slot and
   Audition.
2. **Sharing loop:** upload from the app, deep links, collections (5-preset
   song banks), votes / popular.
3. **Reach:** WASM decoder + Web MIDI audition on the website; email sign-in;
   the API offered to other editors.

## Open questions

- Name: "G-System Vault"? A domain?
- Host on your infrastructure or a managed platform? The home lab is fine for
  staging, but a public service wants a VM or Fly.io.
- Should firmware version be required? Presets from older firmware store fewer
  parameters; the decoder handles it, but it's useful to show.
- Moderation load is unknown. Start with report-to-hide plus manual review.
- Allow anonymous uploads? Probably not: accounts make abuse handling and
  attribution simple.
