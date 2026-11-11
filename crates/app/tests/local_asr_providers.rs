use template_app::{
    MOONSHINE_PROVIDER_ID, ProviderCatalog, SEACO_PROVIDER_ID, ZIPFORMER_PROVIDER_ID,
};

#[test]
fn new_local_provider_selections_restore_independently_and_remain_exclusive() {
    let mut catalog = ProviderCatalog::default();
    catalog.select_whisper_provider();
    catalog.select_qwen3_asr_provider();
    catalog.select_sense_voice_provider();
    let existing = catalog.asr_providers.clone();

    catalog.select_seaco_provider();
    catalog.select_seaco_provider();
    assert!(catalog.seaco_is_active());
    assert!(!catalog.sense_voice_is_active());
    assert_eq!(Some(SEACO_PROVIDER_ID), catalog.active.asr.as_deref());
    assert_eq!("seaco-paraformer", catalog.asr_providers[3].provider_type);

    catalog.select_zipformer_provider();
    assert!(catalog.zipformer_is_active());
    assert!(!catalog.seaco_is_active());
    assert!(!catalog.clear_seaco_selection());
    assert_eq!(Some(ZIPFORMER_PROVIDER_ID), catalog.active.asr.as_deref());
    assert_eq!("zipformer", catalog.asr_providers[4].provider_type);

    catalog.select_moonshine_provider();
    assert!(catalog.moonshine_is_active());
    assert!(!catalog.zipformer_is_active());
    assert!(!catalog.clear_zipformer_selection());
    assert_eq!(Some(MOONSHINE_PROVIDER_ID), catalog.active.asr.as_deref());
    assert_eq!("moonshine", catalog.asr_providers[5].provider_type);
    assert_eq!(existing, catalog.asr_providers[..3]);
    assert_eq!(6, catalog.asr_providers.len());

    let mut restored = ProviderCatalog {
        active: catalog.active.clone(),
        asr_providers: catalog.asr_providers.clone(),
        llm_providers: catalog.llm_providers.clone(),
    };
    assert!(restored.moonshine_is_active());
    assert!(restored.clear_moonshine_selection());
    assert!(!restored.clear_moonshine_selection());
    restored.select_seaco_provider();
    assert!(restored.clear_seaco_selection());
    restored.select_zipformer_provider();
    assert!(restored.clear_zipformer_selection());
    assert_eq!(catalog.asr_providers, restored.asr_providers);
}

#[test]
fn a_provider_with_a_mismatched_type_is_not_restored_as_a_local_model() {
    let mut catalog = ProviderCatalog::default();
    for select in [
        ProviderCatalog::select_seaco_provider,
        ProviderCatalog::select_zipformer_provider,
        ProviderCatalog::select_moonshine_provider,
    ] {
        select(&mut catalog);
        assert!(!catalog.asr_providers.is_empty());
        for provider in &mut catalog.asr_providers {
            provider.provider_type = "unknown".to_owned();
        }
        assert!(!catalog.seaco_is_active());
        assert!(!catalog.zipformer_is_active());
        assert!(!catalog.moonshine_is_active());
    }
}
