use rusqlite::{OptionalExtension, Transaction, params};
use template_app::{
    CandidateAssessmentSource, CandidateDecision, DictionaryCandidateAssessment,
    DictionaryCandidateEvidence, DictionaryCandidateKind, DictionaryCandidateState,
    DictionaryLearningOutcome, DictionaryOrigin, NewDictionaryEntry, NewDictionaryObservation,
    StorageError, dictionary_comparison_key, normalize_language_tag,
};

use super::{dictionary, unavailable};

const OBSERVATION_WINDOW_MS: i64 = 30 * 24 * 60 * 60 * 1_000;

pub(super) fn record(
    connection: &mut rusqlite::Connection,
    observation: NewDictionaryObservation,
) -> Result<DictionaryLearningOutcome, StorageError> {
    let normalized = NormalizedObservation::new(observation)?;
    let Some((required_occurrences, required_dictations)) =
        normalized.assessment.required_evidence()
    else {
        return Ok(DictionaryLearningOutcome::Rejected);
    };
    let transaction = connection.transaction().map_err(unavailable)?;
    if is_suppressed(&transaction, &normalized)? {
        return Ok(DictionaryLearningOutcome::Suppressed);
    }
    let (occurrence_count, dictation_count) = record_evidence(&transaction, &normalized)?;
    if occurrence_count < required_occurrences || dictation_count < required_dictations {
        transaction.commit().map_err(unavailable)?;
        return Ok(DictionaryLearningOutcome::Pending {
            occurrence_count,
            dictation_count,
        });
    }
    let id = promote_candidate(&transaction, &normalized, occurrence_count, dictation_count)?;
    transaction.commit().map_err(unavailable)?;
    dictionary::find_by_id(connection, &id)?
        .map(DictionaryLearningOutcome::Added)
        .ok_or_else(|| {
            StorageError::Invalid("automatic dictionary entry was not created".to_owned())
        })
}

fn record_evidence(
    transaction: &Transaction<'_>,
    normalized: &NormalizedObservation,
) -> Result<(u32, u32), StorageError> {
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
    let (occurrence_count, dictation_count) = aggregate(transaction, normalized)?;
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
    upsert_evidence(
        transaction,
        normalized,
        occurrence_count,
        dictation_count,
        DictionaryCandidateState::Pending,
    )?;
    Ok((occurrence_count, dictation_count))
}

fn promote_candidate(
    transaction: &Transaction<'_>,
    normalized: &NormalizedObservation,
    occurrence_count: u32,
    dictation_count: u32,
) -> Result<String, StorageError> {
    let id = dictionary::upsert_in_transaction(
        transaction,
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
    upsert_evidence(
        transaction,
        normalized,
        occurrence_count,
        dictation_count,
        DictionaryCandidateState::Promoted,
    )?;
    transaction
        .execute(
            "DELETE FROM dictionary_candidates
             WHERE language = ?1 AND canonical_key = ?2",
            params![normalized.language, normalized.canonical_key],
        )
        .map_err(unavailable)?;
    Ok(id)
}

pub(super) fn list_evidence(
    connection: &rusqlite::Connection,
) -> Result<Vec<DictionaryCandidateEvidence>, StorageError> {
    let mut statement = connection
        .prepare(
            "SELECT canonical, language, decision, candidate_kind, confidence,
                    assessment_source, occurrence_count, dictation_count, state,
                    last_observed_at_ms
             FROM dictionary_candidate_evidence
             ORDER BY last_observed_at_ms DESC, canonical_key ASC",
        )
        .map_err(unavailable)?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, u8>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, u32>(6)?,
                row.get::<_, u32>(7)?,
                row.get::<_, String>(8)?,
                row.get::<_, i64>(9)?,
            ))
        })
        .map_err(unavailable)?;
    rows.map(|row| {
        let (
            canonical,
            language,
            decision,
            kind,
            confidence,
            source,
            occurrences,
            dictations,
            state,
            last,
        ) = row.map_err(unavailable)?;
        Ok(DictionaryCandidateEvidence {
            canonical,
            language,
            assessment: DictionaryCandidateAssessment {
                decision: parse_decision(&decision)?,
                kind: parse_kind(&kind)?,
                confidence,
                source: parse_source(&source)?,
            },
            occurrence_count: occurrences,
            dictation_count: dictations,
            state: parse_state(&state)?,
            last_observed_at_ms: last,
        })
    })
    .collect()
}

fn upsert_evidence(
    transaction: &rusqlite::Transaction<'_>,
    observation: &NormalizedObservation,
    occurrence_count: u32,
    dictation_count: u32,
    state: DictionaryCandidateState,
) -> Result<(), StorageError> {
    transaction
        .execute(
            "INSERT INTO dictionary_candidate_evidence(
                language, canonical_key, canonical, decision, candidate_kind,
                confidence, assessment_source, occurrence_count, dictation_count,
                state, last_observed_at_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT(language, canonical_key) DO UPDATE SET
                canonical = excluded.canonical,
                decision = excluded.decision,
                candidate_kind = excluded.candidate_kind,
                confidence = excluded.confidence,
                assessment_source = excluded.assessment_source,
                occurrence_count = excluded.occurrence_count,
                dictation_count = excluded.dictation_count,
                state = excluded.state,
                last_observed_at_ms = excluded.last_observed_at_ms",
            params![
                observation.language,
                observation.canonical_key,
                observation.canonical,
                decision_name(observation.assessment.decision),
                kind_name(observation.assessment.kind),
                observation.assessment.confidence,
                source_name(observation.assessment.source),
                occurrence_count,
                dictation_count,
                state_name(state),
                observation.observed_at_ms,
            ],
        )
        .map(|_| ())
        .map_err(unavailable)
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
    assessment: DictionaryCandidateAssessment,
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
            assessment: observation.assessment,
        })
    }
}

fn decision_name(value: CandidateDecision) -> &'static str {
    match value {
        CandidateDecision::Accept => "accept",
        CandidateDecision::Reject => "reject",
        CandidateDecision::Uncertain => "uncertain",
    }
}

fn kind_name(value: DictionaryCandidateKind) -> &'static str {
    match value {
        DictionaryCandidateKind::NamedTerm => "named_term",
        DictionaryCandidateKind::Acronym => "acronym",
        DictionaryCandidateKind::CodeIdentifier => "code_identifier",
        DictionaryCandidateKind::ProfessionalPhrase => "professional_phrase",
        DictionaryCandidateKind::OrdinaryFragment => "ordinary_fragment",
        DictionaryCandidateKind::Unknown => "unknown",
    }
}

fn source_name(value: CandidateAssessmentSource) -> &'static str {
    match value {
        CandidateAssessmentSource::Local => "local",
        CandidateAssessmentSource::Llm => "llm",
    }
}

fn state_name(value: DictionaryCandidateState) -> &'static str {
    match value {
        DictionaryCandidateState::Pending => "pending",
        DictionaryCandidateState::Promoted => "promoted",
    }
}

fn invalid_evidence(field: &str) -> StorageError {
    StorageError::Invalid(format!("invalid dictionary candidate evidence {field}"))
}

fn parse_decision(value: &str) -> Result<CandidateDecision, StorageError> {
    match value {
        "accept" => Ok(CandidateDecision::Accept),
        "reject" => Ok(CandidateDecision::Reject),
        "uncertain" => Ok(CandidateDecision::Uncertain),
        _ => Err(invalid_evidence("decision")),
    }
}

fn parse_kind(value: &str) -> Result<DictionaryCandidateKind, StorageError> {
    match value {
        "named_term" => Ok(DictionaryCandidateKind::NamedTerm),
        "acronym" => Ok(DictionaryCandidateKind::Acronym),
        "code_identifier" => Ok(DictionaryCandidateKind::CodeIdentifier),
        "professional_phrase" => Ok(DictionaryCandidateKind::ProfessionalPhrase),
        "ordinary_fragment" => Ok(DictionaryCandidateKind::OrdinaryFragment),
        "unknown" => Ok(DictionaryCandidateKind::Unknown),
        _ => Err(invalid_evidence("kind")),
    }
}

fn parse_source(value: &str) -> Result<CandidateAssessmentSource, StorageError> {
    match value {
        "local" => Ok(CandidateAssessmentSource::Local),
        "llm" => Ok(CandidateAssessmentSource::Llm),
        _ => Err(invalid_evidence("source")),
    }
}

fn parse_state(value: &str) -> Result<DictionaryCandidateState, StorageError> {
    match value {
        "pending" => Ok(DictionaryCandidateState::Pending),
        "promoted" => Ok(DictionaryCandidateState::Promoted),
        _ => Err(invalid_evidence("state")),
    }
}
