use rusqlite::{OptionalExtension, params};
use template_app::{
    DictionaryLearningOutcome, DictionaryOrigin, NewDictionaryEntry, NewDictionaryObservation,
    StorageError, dictionary_comparison_key, normalize_language_tag,
};

use super::{dictionary, unavailable};

const OBSERVATION_WINDOW_MS: i64 = 30 * 24 * 60 * 60 * 1_000;
const REQUIRED_OCCURRENCES: u32 = 3;
const REQUIRED_DICTATIONS: u32 = 2;

pub(super) fn record(
    connection: &mut rusqlite::Connection,
    observation: NewDictionaryObservation,
) -> Result<DictionaryLearningOutcome, StorageError> {
    let normalized = NormalizedObservation::new(observation)?;
    let transaction = connection.transaction().map_err(unavailable)?;
    if is_suppressed(&transaction, &normalized)? {
        return Ok(DictionaryLearningOutcome::Suppressed);
    }
    transaction
        .execute(
            "DELETE FROM term_observations WHERE observed_at_ms < ?1",
            [normalized
                .observed_at_ms
                .saturating_sub(OBSERVATION_WINDOW_MS)],
        )
        .map_err(unavailable)?;
    transaction
        .execute(
            "INSERT INTO term_observations(
                dictation_id, language, canonical, canonical_key,
                occurrence_count, observed_at_ms
             ) VALUES (?1, ?2, ?3, ?4, 1, ?5)
             ON CONFLICT(dictation_id, language, canonical_key) DO UPDATE SET
                occurrence_count = term_observations.occurrence_count + 1,
                observed_at_ms = excluded.observed_at_ms",
            params![
                normalized.dictation_id,
                normalized.language,
                normalized.canonical,
                normalized.canonical_key,
                normalized.observed_at_ms,
            ],
        )
        .map_err(unavailable)?;
    let (occurrence_count, dictation_count) = aggregate(&transaction, &normalized)?;
    transaction
        .execute(
            "INSERT INTO dictionary_candidates(
                language, canonical_key, occurrence_count,
                dictation_count, last_observed_at_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(language, canonical_key) DO UPDATE SET
                occurrence_count = excluded.occurrence_count,
                dictation_count = excluded.dictation_count,
                last_observed_at_ms = excluded.last_observed_at_ms",
            params![
                normalized.language,
                normalized.canonical_key,
                occurrence_count,
                dictation_count,
                normalized.observed_at_ms,
            ],
        )
        .map_err(unavailable)?;
    if occurrence_count < REQUIRED_OCCURRENCES || dictation_count < REQUIRED_DICTATIONS {
        transaction.commit().map_err(unavailable)?;
        return Ok(DictionaryLearningOutcome::Pending {
            occurrence_count,
            dictation_count,
        });
    }
    let id = dictionary::upsert_in_transaction(
        &transaction,
        NewDictionaryEntry {
            canonical: normalized.canonical.clone(),
            language: normalized.language.clone(),
            origin: DictionaryOrigin::Automatic,
        },
        normalized.observed_at_ms,
    )?;
    transaction
        .execute(
            "DELETE FROM term_observations
             WHERE language = ?1 AND canonical_key = ?2",
            params![normalized.language, normalized.canonical_key],
        )
        .map_err(unavailable)?;
    transaction
        .execute(
            "DELETE FROM dictionary_candidates
             WHERE language = ?1 AND canonical_key = ?2",
            params![normalized.language, normalized.canonical_key],
        )
        .map_err(unavailable)?;
    transaction.commit().map_err(unavailable)?;
    dictionary::find_by_id(connection, &id)?
        .map(DictionaryLearningOutcome::Added)
        .ok_or_else(|| {
            StorageError::Invalid("automatic dictionary entry was not created".to_owned())
        })
}

fn aggregate(
    transaction: &rusqlite::Transaction<'_>,
    observation: &NormalizedObservation,
) -> Result<(u32, u32), StorageError> {
    transaction
        .query_row(
            "SELECT COALESCE(SUM(occurrence_count), 0), COUNT(DISTINCT dictation_id)
             FROM term_observations
             WHERE language = ?1 AND canonical_key = ?2",
            params![observation.language, observation.canonical_key],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(unavailable)
}

fn is_suppressed(
    transaction: &rusqlite::Transaction<'_>,
    observation: &NormalizedObservation,
) -> Result<bool, StorageError> {
    transaction
        .query_row(
            "SELECT suppressed_until_ms FROM dictionary_suppressions
             WHERE language = ?1 AND canonical_key = ?2",
            params![observation.language, observation.canonical_key],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map(|until| until.is_some_and(|until| until > observation.observed_at_ms))
        .map_err(unavailable)
}

struct NormalizedObservation {
    dictation_id: String,
    language: String,
    canonical: String,
    canonical_key: String,
    observed_at_ms: i64,
}

impl NormalizedObservation {
    fn new(observation: NewDictionaryObservation) -> Result<Self, StorageError> {
        let dictation_id = observation.dictation_id.trim().to_owned();
        let canonical = observation.correction.canonical.trim().to_owned();
        if dictation_id.is_empty() || canonical.is_empty() {
            return Err(StorageError::Invalid(
                "dictionary observation fields must not be empty".to_owned(),
            ));
        }
        Ok(Self {
            dictation_id,
            language: normalize_language_tag(&observation.language)?,
            canonical_key: dictionary_comparison_key(&canonical),
            canonical,
            observed_at_ms: observation.observed_at_ms,
        })
    }
}
