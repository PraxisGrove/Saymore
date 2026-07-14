# ADR 0004: Use SQLite with application-encrypted history

- Status: Accepted
- Date: 2026-07-14

## Context

Saymore needs durable local settings, final-output history, a personal dictionary,
automatic term observations, trigger bindings, and installed-model metadata. The
history contains private user text, while the dictionary must remain usable if the
history key is unavailable. Provider configuration is versioned independently and
can contain several ASR and LLM instances with different JSON shapes.

The desktop UI must not block on filesystem, credential-vault, encryption, or
database work. macOS and Windows must use the same database and crypto format.

## Decision

Use one fixed `saymore.sqlite3` file with separate tables for separate business
concepts. SQLite is the source of truth for local product data. Provider instances
remain in `config.json`; original audio does not persist in the current release.

The database lives at:

- macOS: `~/Library/Application Support/Saymore/saymore.sqlite3`
- Windows: `%LOCALAPPDATA%\Saymore\saymore.sqlite3`

The location is not user-configurable and must not be moved into a cloud-sync
directory. A future audio library is separate and may use a user-selected volume.

### Connection ownership

One bounded worker owns one `rusqlite::Connection`. Callers use typed application
ports and never receive a connection or execute SQL. The worker queue is bounded to
prevent memory growth; this is backpressure, not idempotency. A stable dictation
UUID separately makes history insertion idempotent.

Saymore also holds an operating-system file lock beside the database for the full
process lifetime. A second process refuses to start, so the one-worker ownership
model applies to the whole application rather than only one process.

Open the database with:

- bundled SQLite through `rusqlite`;
- WAL journal mode;
- foreign keys enabled;
- `synchronous = FULL`;
- finite `busy_timeout`;
- `secure_delete = ON`;
- `integrity_check` before writes;
- forward-only transactional migrations through `PRAGMA user_version`.

An older application refuses to write a database whose schema version is newer.
Corruption stops writes and preserves the original file for explicit recovery or
reset. Saymore does not create hidden database backups.

### Tables

| Table | Chinese name | Responsibility |
|---|---|---|
| `app_settings` | 应用设置 | Typed, implemented product settings such as history retention |
| `trigger_bindings` | 触发器绑定 | Stable keyboard or mouse trigger definitions |
| `dictionary_entries` | 词典词条 | Standard spelling, normalized key, language, origin, and timestamps |
| `dictionary_variants` | 识别变体 | Compatibility data reserved for a future high-confidence correction design; not used by the current runtime |
| `term_observations` | 词汇观察 | Compatibility data reserved for future high-confidence user correction signals; current runtime does not write it |
| `dictionary_candidates` | 候选聚合 | Compatibility data reserved for future automatic dictionary addition; it is not a current user review queue |
| `dictionary_suppressions` | 词典静默 | Compatibility data reserved for a future undo and suppression policy |
| `transcript_history` | 听写历史 | UUID, time, crypto versions, nonce, and encrypted payload |
| `history_key_validation` | 历史密钥校验 | Authenticated sentinel used to distinguish the wrong key from a corrupt history row |
| `installed_models` | 已安装模型 | Metadata only for models already installed by trusted runtime flows; no Model Hub behavior |

### History encryption

Encrypt each history payload with AES-256-GCM and a fresh random 96-bit nonce.
The 32-byte random data key is stored in macOS Keychain or Windows Credential
Manager through the `SecretStore` port. Never store it in SQLite or silently fall
back to plaintext.

In normal release builds, the encrypted payload contains only the final best text
and essential metadata: audio duration, language when known, delivery status,
refinement status, and Provider instance IDs. It excludes audio, raw ASR text,
intermediate text, LLM responses, API keys, and complete error responses.

Debug builds, or release builds compiled with the explicit
`history-experiments` feature, may additionally include the raw ASR text and the
accepted LLM-refined text in the same encrypted payload for local
ASR-versus-LLM-versus-final comparison. Both fields are optional and omitted from
normal release builds; they are never stored in plaintext columns or uploaded.

Use the history UUID, UTC creation time, and payload version as authenticated
additional data. Store `crypto_version`, `payload_version`, nonce, and ciphertext
in explicit columns so future readers can migrate safely.

Opening the database, loading settings or dictionary data, and retention cleanup
do not access the credential vault. Resolve the history key lazily on the first
encrypted history read, write, or reset.

Key lifecycle:

1. Empty history plus missing key creates and stores a new key.
2. Existing rows plus a missing, invalid, or non-authenticating key lock history.
3. A temporarily unavailable credential service returns an unavailable error, not
   a permanent-missing result.
4. Settings and dictionary operations remain available while history is locked.
5. Explicit history reset deletes old rows, rotates the key, and writes a new
   validation sentinel.

Encryption prevents another process that only obtains the database file from
reading transcript contents. It does not protect against compromise of the same
OS account, an administrator, or plaintext already present in application memory.

### History behavior

After a usable final result exists, preflight the currently focused target's privacy
on the UI thread. A sensitive or unknown secure target never creates a history row.
For a standard target, create the encrypted history row on the bounded storage
worker before attempting delivery and update the encrypted delivery status
afterward. If the target becomes secure between preflight and delivery, delete the
provisional record. Cancellation, empty speech, history disabled, and failed
recognition do not create a record.

History is enabled by default with seven-day retention. Choices are one day, seven
days, thirty days, and permanent. Cleanup runs after migrations at startup, in the
same settings workflow when retention is shortened, after successful inserts, and
every 24 hours while resident. Permanent retention has no hidden capacity eviction;
disk-full errors are visible and do not block text delivery.

Read newest first in batches of at most 50 with the keyset cursor
`(created_at_ms, UUID)`. Do not use offset pagination. A single delete is delayed
for the three-second UI undo window; clear-all requires confirmation. After actual
deletion, checkpoint and truncate WAL data.

### Dictionary files and learning

CSV is import-only and accepts at most two columns: canonical spelling and optional
language. Imports are additive, preserve an existing display spelling, and reject
the legacy third variants column. There is no Markdown projection or dictionary
export; SQLite remains the only source of truth.

Canonical identity preserves token boundaries: `open ai` and `openai` are distinct,
while case, Unicode width, and repeated whitespace remain normalized for duplicate
detection. Schema version 4 recomputes canonical keys for existing confirmed entries
without changing legacy observations, candidates, suppressions, or variants.

Automatic dictionary addition remains a long-term core product capability. The
current ASR-versus-LLM/final diff mechanism is disabled because a model rewrite is
not a high-confidence user correction signal. The runtime does not create new term
observations, candidates, suppressions, or automatic entries, and does not show the
old undo toast. Existing tables and compatible rows remain in place so migrations
do not destroy user data. Re-enabling automatic addition requires a separately
designed high-confidence user correction signal, confidence policy, privacy model,
and undo behavior.

The current runtime only supplies confirmed entries whose canonical spelling occurs
in the transcript in a case- or width-equivalent form, with a maximum of 50 entries.
It strips stored recognition variants before the LLM request. After optional LLM
refinement, deterministic normalization applies canonical spelling only to complete
ASCII alphanumeric tokens and protects URLs, email addresses, paths, domains, and
underscore identifiers. It does not infer aliases such as `open ai -> OpenAI`.

## Consequences

SQLite gives transactions, constraints, migrations, pagination, and reliable
cross-table cleanup, but the database is not directly editable. Manual UI editing
and CSV import provide the current dictionary management surface without creating a
second source of truth. Application-layer encryption permits plaintext dictionary
queries while protecting transcript payloads, at the cost of key lifecycle and
corruption UI.

Provider JSON and SQLite have separate migration paths. This is intentional:
Provider-specific unknown JSON remains round-trippable, while typed product data
gets database constraints.
