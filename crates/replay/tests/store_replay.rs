use bm_core::{
    MemoryPlane, MemoryRecord, NewMemoryRecord, ProjectionBlock, ProjectionSurface,
    PromptRecallIntent, RecallQuery, RecallSelectionReport, RuntimeProfile, WriteCandidate,
    WriteDecision,
};
use bm_sdk::MemoryRuntimeBuilder;
use bm_store::{FileStore, MemoryStore};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn persistent_replay_recovers_recall_and_projection_after_reopen() {
    let root = TempStoreRoot::new("persistent_replay_recovers_recall_and_projection_after_reopen");

    let before = {
        let store = FileStore::open(root.path()).expect("open empty file store");
        let mut runtime = MemoryRuntimeBuilder::new(RuntimeProfile::DevFull)
            .store(store)
            .build();

        for candidate in replay_candidates() {
            let report = runtime.write(candidate);
            assert_eq!(report.decision, WriteDecision::Accepted);
        }

        let recall = runtime.recall(replay_query());
        assert_eq!(recall.selected.len(), 3);
        let projection = runtime.project(&recall, ProjectionSurface::Prompt);

        (
            selection_fingerprint(&recall),
            projection_fingerprint(&projection.blocks),
        )
    };

    let after = {
        let store = FileStore::open(root.path()).expect("reopen persisted file store");
        let runtime = MemoryRuntimeBuilder::new(RuntimeProfile::DevFull)
            .store(store)
            .build();
        let recall = runtime.recall(replay_query());
        let projection = runtime.project(&recall, ProjectionSurface::Prompt);

        (
            selection_fingerprint(&recall),
            projection_fingerprint(&projection.blocks),
        )
    };

    assert_eq!(after, before);
}

#[test]
fn event_log_replay_keeps_sequences_contiguous_and_snapshot_reopen_does_not_duplicate_records() {
    let root = TempStoreRoot::new(
        "event_log_replay_keeps_sequences_contiguous_and_snapshot_reopen_does_not_duplicate_records",
    );

    {
        let mut store = FileStore::open(root.path()).expect("open empty file store");
        store
            .insert(new_record(
                MemoryPlane::SharedFactual,
                "persistent replay fact",
                "replay:s2:first",
            ))
            .expect("insert first replay record");
        store
            .insert(new_record(
                MemoryPlane::Procedural,
                "下次验证持久化 replay 时，先重开 store 再比较 projection。",
                "task-learning:s2",
            ))
            .expect("insert second replay record");
    }

    assert_event_log_sequences(root.path(), &[1, 2]);

    {
        let mut reopened = FileStore::open(root.path()).expect("replay event log");
        let records = reopened.records().expect("records after event replay");
        assert_record_ids(&records, &["mem-1", "mem-2"]);

        let snapshot = reopened.snapshot().expect("snapshot replayed records");
        assert_eq!(snapshot.schema_version, 1);
        assert_eq!(snapshot.snapshot_event_seq, 2);
        assert_eq!(snapshot.record_count, 2);
    }

    let after_snapshot = FileStore::open(root.path()).expect("reopen after snapshot");
    let records = after_snapshot
        .records()
        .expect("records after snapshot replay");

    assert_record_ids(&records, &["mem-1", "mem-2"]);
    assert_eq!(unique_id_count(&records), records.len());
}

fn replay_candidates() -> Vec<WriteCandidate> {
    vec![
        WriteCandidate::new("agent:s2", "task:s2:replay", "项目名称是 Beetle Memory")
            .source("replay:s2:factual")
            .plane_hint(MemoryPlane::SharedFactual),
        WriteCandidate::new(
            "agent:s2",
            "task:s2:replay",
            "archive hit remains evidence until distillation",
        )
        .source("archive:s2-hit")
        .plane_hint(MemoryPlane::ArchiveEvidence),
        WriteCandidate::new(
            "agent:s2",
            "task:s2:replay",
            "当前回合使用 compact 主体挂载帧；私域原文已过滤。",
        )
        .source("host:subject-state")
        .plane_hint(MemoryPlane::SubjectProjection),
    ]
}

fn replay_query() -> RecallQuery {
    RecallQuery::new("task:s2:replay")
        .identity("agent:s2")
        .intent(PromptRecallIntent::Mixed)
        .limit(8)
}

fn new_record(plane: MemoryPlane, content: &str, source: &str) -> NewMemoryRecord {
    NewMemoryRecord {
        identity: "agent:s2".to_owned(),
        scope: "task:s2:replay-seq".to_owned(),
        content: content.to_owned(),
        source: source.to_owned(),
        domain: plane.domain(),
        plane,
    }
}

fn selection_fingerprint(report: &RecallSelectionReport) -> Vec<(String, String, String, String)> {
    let mut fingerprint = report
        .selected
        .iter()
        .map(|selection| {
            (
                selection.record_id.clone(),
                plane_name(selection.plane).to_owned(),
                selection.source.id.clone(),
                selection.content.clone(),
            )
        })
        .collect::<Vec<_>>();
    fingerprint.sort();
    fingerprint
}

fn projection_fingerprint(
    blocks: &[ProjectionBlock],
) -> Vec<(String, String, String, String, bool)> {
    let mut fingerprint = blocks
        .iter()
        .map(|block| {
            (
                block.record_id.clone(),
                plane_name(block.plane).to_owned(),
                block.source.id.clone(),
                block.content.clone(),
                block.privacy_filtered,
            )
        })
        .collect::<Vec<_>>();
    fingerprint.sort();
    fingerprint
}

fn assert_event_log_sequences(root: &Path, expected_sequences: &[u64]) {
    let events = fs::read_to_string(root.join("events.jsonl")).expect("read events.jsonl");
    let lines = events
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>();

    assert_eq!(lines.len(), expected_sequences.len());
    for (line, expected) in lines.iter().zip(expected_sequences) {
        assert!(
            json_line_contains_seq(line, *expected),
            "expected seq {expected} in event line: {line}"
        );
    }
}

fn json_line_contains_seq(line: &str, seq: u64) -> bool {
    line.contains(&format!("\"seq\":{seq}")) || line.contains(&format!("\"seq\": {seq}"))
}

fn assert_record_ids(records: &[MemoryRecord], expected: &[&str]) {
    let ids = records
        .iter()
        .map(|record| record.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(ids, expected);
}

fn unique_id_count(records: &[MemoryRecord]) -> usize {
    records
        .iter()
        .map(|record| record.id.as_str())
        .collect::<HashSet<_>>()
        .len()
}

fn plane_name(plane: MemoryPlane) -> &'static str {
    match plane {
        MemoryPlane::SharedFactual => "SharedFactual",
        MemoryPlane::Procedural => "Procedural",
        MemoryPlane::ContinuityCapsule => "ContinuityCapsule",
        MemoryPlane::ArchiveEvidence => "ArchiveEvidence",
        MemoryPlane::TaskRecall => "TaskRecall",
        MemoryPlane::SubjectProjection => "SubjectProjection",
        MemoryPlane::SoulGovernance => "SoulGovernance",
    }
}

struct TempStoreRoot {
    path: PathBuf,
}

impl TempStoreRoot {
    fn new(test_name: &str) -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time after epoch")
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("bm-s2-{test_name}-{}-{unique}", std::process::id()));
        fs::create_dir_all(&path).expect("create temp store root");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempStoreRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
