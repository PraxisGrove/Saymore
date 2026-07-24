# ADR 0004: Use SQLite with application-encrypted history

- Status: Accepted
- Date: 2026-07-14

## Context

Saymore needs durable local settings, final-output history, a personal
dictionary, automatic term observations, trigger bindings, and installed-model
metadata. The history contains private user text, while the dictionary must
remain usable if the history key is unavailable. Provider configuration is
versioned independently and can contain several ASR and LLM instances with
different JSON shapes.

The desktop UI must not block on filesystem, credential-vault, encryption, or
database work. macOS and Windows must use the same database and crypto format.

## Decision

Use one fixed `saymore.sqlite3` file with separate tables for separate business
concepts. SQLite is the source of truth for local product data. Provider
instances remain in `config.json`; original audio does not persist in the
current release.

The database lives at:

- macOS: `~/Library/Application Support/Saymore/saymore.sqlite3`
- Windows: `%LOCALAPPDATA%\Saymore\saymore.sqlite3`

The location is not user-configurable and must not be moved into a cloud-sync
directory. A future audio library is separate and may use a user-selected
volume.

### Connection ownership

One bounded worker owns one `rusqlite::Connection`. Callers use typed
application ports and never receive a connection or execute SQL. The worker
queue is bounded to prevent memory growth; this is backpressure, not
idempotency. A stable dictation UUID separately makes history insertion
idempotent.

Saymore also holds an operating-system file lock beside the database for the
full process lifetime. A second process refuses to start, so the one-worker
ownership model applies to the whole application rather than only one process.

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

| Table                     | Chinese name | Responsibility                                                                                                |
| ------------------------- | ------------ | ------------------------------------------------------------------------------------------------------------- |
| `app_settings`            | 应用设置     | Typed, implemented product settings such as history retention                                                 |
| `trigger_bindings`        | 触发器绑定   | Stable keyboard or mouse trigger definitions                                                                  |
| `dictionary_entries`      | 词典词条     | Standard spelling, normalized key, language, origin, and timestamps                                           |
| `dictionary_variants`     | 识别变体     | Compatibility data only; the runtime does not use variants as deterministic replacement mappings              |
| `term_observations`       | 词汇观察     | Minimal evidence from user edits to the local range Saymore just delivered                                    |
| `dictionary_candidates`   | 候选聚合     | Internal evidence counts and decision state for automatic dictionary learning; not a normal user review queue |
| `dictionary_suppressions` | 词典静默     | Prevents a deleted automatic term from being learned again without explicit user authorization                |
| `transcript_history`      | 听写历史     | UUID, time, crypto versions, nonce, and encrypted payload                                                     |
| `history_key_validation`  | 历史密钥校验 | Authenticated sentinel used to distinguish the wrong key from a corrupt history row                           |
| `installed_models`        | 已安装模型   | Metadata only for models already installed by trusted runtime flows; no model catalog or marketplace behavior |

### History encryption

Encrypt each history payload with AES-256-GCM and a fresh random 96-bit nonce.
The 32-byte random data key is stored in macOS Keychain or Windows Credential
Manager through the `SecretStore` port. Never store it in SQLite or silently
fall back to plaintext.

In normal release builds, the encrypted payload contains only the final best
text and essential metadata: audio duration, language when known, delivery
status, refinement status, and Provider instance IDs. It excludes audio, raw ASR
text, intermediate text, LLM responses, API keys, and complete error responses.

Debug builds, or release builds compiled with the explicit `history-experiments`
feature, may additionally include the raw ASR text and the accepted LLM-refined
text in the same encrypted payload for local ASR-versus-LLM-versus-final
comparison. Both fields are optional and omitted from normal release builds;
they are never stored in plaintext columns or uploaded.

Use the history UUID, UTC creation time, and payload version as authenticated
additional data. Store `crypto_version`, `payload_version`, nonce, and
ciphertext in explicit columns so future readers can migrate safely.

Opening the database, loading settings or dictionary data, and retention cleanup
do not access the credential vault. Resolve the history key lazily on the first
encrypted history read, write, or reset.

Key lifecycle:

1. Empty history plus missing key creates and stores a new key.
2. Existing rows plus a missing, invalid, or non-authenticating key lock
   history.
3. A temporarily unavailable credential service returns an unavailable error,
   not a permanent-missing result.
4. Settings and dictionary operations remain available while history is locked.
5. Explicit history reset deletes old rows, rotates the key, and writes a new
   validation sentinel.

Encryption prevents another process that only obtains the database file from
reading transcript contents. It does not protect against compromise of the same
OS account, an administrator, or plaintext already present in application
memory.

### History behavior

After a usable final result exists, attempt delivery before inserting history. A
restricted paste to a sensitive target keeps its transcript and clipboard
snapshot transient and never creates a history row, regardless of whether the
unverified paste succeeds. Standard targets create one encrypted history row
after the delivery outcome is known, with the corresponding delivered or
not-delivered status. Cancellation, empty speech, history disabled, and failed
recognition do not create a record.

History is enabled by default with seven-day retention. Choices are one day,
seven days, thirty days, and permanent. Cleanup runs after migrations at
startup, in the same settings workflow when retention is shortened, after
successful inserts, and every 24 hours while resident. Permanent retention has
no hidden capacity eviction; disk-full errors are visible and do not block text
delivery.

Read newest first in batches of at most 50 with the keyset cursor
`(created_at_ms, UUID)`. Do not use offset pagination. A single delete is
delayed for the three-second UI undo window; clear-all requires confirmation.
After actual deletion, checkpoint and truncate WAL data.

### Dictionary files and learning

CSV is import-only and accepts at most two columns: canonical spelling and
optional language. Imports are additive, preserve an existing display spelling,
and reject the legacy third variants column. There is no Markdown projection or
dictionary export; SQLite remains the only source of truth.

Canonical identity preserves token boundaries: `open ai` and `openai` are
distinct, while case, Unicode width, and repeated whitespace remain normalized
for duplicate detection. Schema version 4 recomputes canonical keys for existing
confirmed entries without changing legacy observations, candidates,
suppressions, or variants.

Automatic dictionary addition is an active core product capability. The retired
ASR-versus-LLM/final diff mechanism remains disabled because a model rewrite is
not a user correction signal. The active signal is a stable local edit made by
the user to the range Saymore just delivered in the same editable control. The
runtime may persist the resulting canonical candidate, evidence counts, decision
metadata, and timestamps, but not the complete input field or surrounding
document.

Automatic promotion uses confidence bands rather than treating every short edit
as a term. Local privacy and attribution checks run first. A dedicated
dictionary candidate classifier may then perform sentence-structure analysis,
candidate extraction, part-of-speech/type classification, and reusable-term
judgment in one structured call. It is separate from the transcription
refinement prompt and is one signal rather than the sole authority: strong
identifier/name morphology and repeated independent corrections must still work
without a configured LLM, while ambiguous candidates require more evidence.
High-confidence candidates are added automatically; medium-confidence evidence
remains internal and is visible only in Development diagnostics; low-confidence
sentence edits are rejected.

Cloud classification requires an explicit Provider data confirmation for the
minimal correction fragment and bounded local context. The classifier never
receives the full input control by default. Deleting an automatic entry creates
suppression, and manually adding the same term clears that suppression. No
implicit observation creates a deterministic error-form-to-canonical replacement
rule.

The current runtime only supplies confirmed entries whose canonical spelling
occurs in the transcript in a case- or width-equivalent form, with a maximum of
50 entries. It strips stored recognition variants before the LLM request. After
optional LLM refinement, deterministic normalization applies canonical spelling
only to complete ASCII alphanumeric tokens and protects URLs, email addresses,
paths, domains, and underscore identifiers. It does not infer aliases such as
`open ai -> OpenAI`.

## Consequences

SQLite gives transactions, constraints, migrations, pagination, and reliable
cross-table cleanup, but the database is not directly editable. Manual UI
editing and CSV import provide the current dictionary management surface without
creating a second source of truth. Application-layer encryption permits
plaintext dictionary queries while protecting transcript payloads, at the cost
of key lifecycle and corruption UI.

Provider JSON and SQLite have separate migration paths. This is intentional:
Provider-specific unknown JSON remains round-trippable, while typed product data
gets database constraints.
