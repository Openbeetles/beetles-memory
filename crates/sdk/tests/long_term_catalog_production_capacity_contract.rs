#![cfg(all(
    target_os = "macos",
    feature = "profile-desktop-macos-standalone-memory",
    not(feature = "nonproduction-replay-harness")
))]

use bm_sdk::{
    primary_human_subject_id, ErrorClass, LongTermMemoryDraft, LongTermMemoryKind,
    LongTermMemoryProvenance, LongTermMemoryQuery, MemoryEvidenceAuthority, MemoryIdentity,
    MemoryLongTermControlView, MemoryLongTermDetailRequest, MemoryLongTermListRequest,
    MemoryLongTermMutation, MemoryLongTermMutationRequest, MemoryLongTermTarget,
    MemoryPrivacyClass, MemoryRuntime, MemoryScope, MemoryStoreHandle,
    MemorySubjectVisibilityPolicy, MemoryWriteRequest, ParsedLongTermMemoryExtraction, ProfileId,
    RuntimeLifecycleModeInput, StoreBackendConfig,
};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn draft(index: usize) -> LongTermMemoryDraft {
    LongTermMemoryDraft {
        kind: LongTermMemoryKind::Fact,
        topic: format!("production_catalog_capacity_{index:02}"),
        content: format!("Synthetic accepted fact number {index} for catalog pagination."),
        keywords: vec!["catalog".into()],
        privacy: MemoryPrivacyClass::SharedWithSubject,
        source_chat_id: None,
        source_type: None,
        source_scope: None,
        subject_visibility: MemorySubjectVisibilityPolicy::AllSubjects,
        provenance: LongTermMemoryProvenance::new(MemoryEvidenceAuthority::UserAsserted),
        confidence: None,
        freshness: None,
        stale_hint: None,
        supporting_citations: Vec::new(),
        canonical_entities: Vec::new(),
        evidence_count: None,
        observed_at: None,
        source_revision: None,
    }
}

fn write(runtime: &MemoryRuntime, draft: LongTermMemoryDraft) -> String {
    let id = draft.stable_id().expect("stable factual owner id");
    let report = runtime
        .write(MemoryWriteRequest::LongTermExtraction {
            extraction: ParsedLongTermMemoryExtraction {
                upserts: vec![draft],
                deletes: Vec::new(),
                skill_writes: Vec::new(),
            },
        })
        .expect("official accepted long-term write");
    assert!(report.accepted, "{report:#?}");
    assert_eq!(report.changed, 1, "{report:#?}");
    id
}

fn page(runtime: &MemoryRuntime, cursor: Option<String>) -> bm_sdk::MemoryLongTermListReport {
    runtime
        .list_long_term_memory(MemoryLongTermListRequest {
            query: LongTermMemoryQuery {
                kind: Some(LongTermMemoryKind::Fact),
                ..LongTermMemoryQuery::default()
            },
            cursor,
            limit: 20,
            view: MemoryLongTermControlView::Operator,
        })
        .expect("official catalog page")
}

fn runtime(store: MemoryStoreHandle) -> MemoryRuntime {
    let profile = ProfileId::DesktopMacosStandaloneMemory;
    let runtime = MemoryRuntime::builder()
        .identity(MemoryIdentity::new("catalog-agent", "catalog-owner").expect("identity"))
        .scope(MemoryScope::new("sdk.direct", "catalog-capacity").expect("scope"))
        .store(store)
        .build()
        .expect("production profile runtime");
    assert_eq!(runtime.capabilities().profile, profile);
    runtime
}

fn seed_43(runtime: &MemoryRuntime) -> BTreeSet<String> {
    let expected = (0..43)
        .map(|index| write(runtime, draft(index)))
        .collect::<BTreeSet<_>>();
    assert_eq!(expected.len(), 43);
    expected
}

fn seed_terminal_and_hidden_noise(runtime: &MemoryRuntime) -> (String, String) {
    let mut terminal = draft(100);
    terminal.kind = LongTermMemoryKind::Preference;
    terminal.topic = "production_catalog_terminal_preference".into();
    let terminal_id = write(runtime, terminal);
    let deletion = runtime
        .mutate_long_term_memory(MemoryLongTermMutationRequest {
            operation: MemoryLongTermMutation::Delete {
                target: MemoryLongTermTarget::RecordId(terminal_id.clone()),
            },
            reason: "synthetic terminal catalog noise".into(),
            dry_run: false,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("official terminal mutation");
    assert!(deletion.accepted, "{deletion:#?}");

    let mut hidden = draft(101);
    hidden.subject_visibility =
        MemorySubjectVisibilityPolicy::OnlySubjects(vec![primary_human_subject_id(
            "catalog-owner",
        )]);
    let hidden_id = write(runtime, hidden);
    (terminal_id, hidden_id)
}

fn assert_43_pages_and_detail(runtime: &MemoryRuntime, expected: &BTreeSet<String>) -> String {
    let first = page(runtime, None);
    assert_eq!(first.total_visible, 43);
    assert_eq!(first.records.len(), 20);
    let first_cursor = first.next_cursor.clone().expect("second page cursor");
    let second = page(runtime, Some(first_cursor.clone()));
    assert_eq!(second.total_visible, 43);
    assert_eq!(second.records.len(), 20);
    let third = page(runtime, second.next_cursor.clone());
    assert_eq!(third.total_visible, 43);
    assert_eq!(third.records.len(), 3);
    assert!(third.next_cursor.is_none());

    let mut seen = BTreeSet::new();
    for row in first
        .records
        .into_iter()
        .chain(second.records)
        .chain(third.records)
    {
        assert!(seen.insert(row.record.id), "catalog repeated an owner");
    }
    assert_eq!(&seen, expected, "catalog skipped or leaked an owner");

    let exact_id = expected.iter().next().expect("seeded owner").clone();
    let detail = runtime
        .get_long_term_memory(MemoryLongTermDetailRequest {
            target: MemoryLongTermTarget::RecordId(exact_id.clone()),
            view: MemoryLongTermControlView::Operator,
        })
        .expect("exact detail under a large scope root");
    assert_eq!(
        detail.record.as_ref().map(|record| record.id.as_str()),
        Some(exact_id.as_str())
    );
    first_cursor
}

#[test]
fn production_macos_catalog_pages_43_records_and_reads_exact_detail() {
    let profile = ProfileId::DesktopMacosStandaloneMemory;
    let store = MemoryStoreHandle::open(
        StoreBackendConfig::in_memory(profile).expect("production profile store config"),
    )
    .expect("production profile Store");
    let runtime = runtime(store);
    let (terminal_id, hidden_id) = seed_terminal_and_hidden_noise(&runtime);
    let expected = seed_43(&runtime);
    let first_cursor = assert_43_pages_and_detail(&runtime, &expected);

    let terminal = runtime
        .get_long_term_memory(MemoryLongTermDetailRequest {
            target: MemoryLongTermTarget::RecordId(terminal_id),
            view: MemoryLongTermControlView::Operator,
        })
        .expect("terminal detail under a large scope root");
    assert!(terminal.record.is_none());
    assert!(terminal.tombstone.is_some());
    let hidden = runtime
        .get_long_term_memory(MemoryLongTermDetailRequest {
            target: MemoryLongTermTarget::RecordId(hidden_id),
            view: MemoryLongTermControlView::Operator,
        })
        .expect("hidden detail under a large scope root");
    assert!(hidden.record.is_none());
    assert!(hidden.revisions.is_empty());
    assert!(hidden.tombstone.is_none());
    assert!(hidden.transcript_refs.is_empty());

    let mut unrelated = draft(43);
    unrelated.kind = LongTermMemoryKind::Preference;
    unrelated.topic = "production_catalog_unrelated_preference".into();
    write(&runtime, unrelated);
    let unchanged = page(&runtime, Some(first_cursor.clone()));
    assert_eq!(unchanged.total_visible, 43);
    assert_eq!(unchanged.records.len(), 20);

    write(&runtime, draft(44));
    let error = runtime
        .list_long_term_memory(MemoryLongTermListRequest {
            query: LongTermMemoryQuery {
                kind: Some(LongTermMemoryKind::Fact),
                ..LongTermMemoryQuery::default()
            },
            cursor: Some(first_cursor),
            limit: 20,
            view: MemoryLongTermControlView::Operator,
        })
        .expect_err("visible catalog change invalidates an older cursor");
    assert_eq!(error.class(), Some(ErrorClass::Conflict));
    assert_eq!(error.stage(), "long_term_control_cursor");
}

fn temp_root(label: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "bm-production-catalog-{label}-{}-{unique}",
        std::process::id()
    ))
}

fn assert_persistent_reopen(config: StoreBackendConfig, root: PathBuf) {
    let store = MemoryStoreHandle::open(config.clone()).expect("persistent production Store");
    let first_runtime = runtime(store);
    let expected = seed_43(&first_runtime);
    drop(first_runtime);

    let reopened = MemoryStoreHandle::open(config).expect("reopened production Store");
    let reopened_runtime = runtime(reopened);
    assert_43_pages_and_detail(&reopened_runtime, &expected);
    drop(reopened_runtime);
    std::fs::remove_dir_all(root).expect("clean synthetic production Store");
}

#[test]
fn production_macos_file_catalog_survives_reopen() {
    let root = temp_root("file");
    let config = StoreBackendConfig::file(&root, ProfileId::DesktopMacosStandaloneMemory)
        .expect("production file config");
    assert_persistent_reopen(config, root);
}

#[test]
fn production_macos_sqlite_catalog_survives_reopen() {
    let root = temp_root("sqlite");
    std::fs::create_dir_all(&root).expect("synthetic sqlite root");
    let config = StoreBackendConfig::sqlite(
        root.join("memory.sqlite"),
        ProfileId::DesktopMacosStandaloneMemory,
    )
    .expect("production sqlite config");
    assert_persistent_reopen(config, root);
}
