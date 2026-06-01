use bm_core::memory::{
    ArchiveRecordLocator, ArchiveRecordSource, ConversationKey, TranscriptEvidenceRef,
};

#[test]
fn transcript_archive_locator_preserves_conversation_key() {
    let key = ConversationKey::new("space-a", "llm.gateway", "conversation-a").unwrap();
    let locator = ArchiveRecordLocator {
        source: ArchiveRecordSource::Transcript,
        memory_space_id: Some(key.memory_space_id.clone()),
        channel_id: Some(key.channel_id.clone()),
        conversation_id: Some(key.conversation_id.clone()),
        turn_id: Some("turn-a".to_string()),
        chat_id: Some("legacy-chat-a".to_string()),
        message_id: Some("message-a".to_string()),
        message_index: Some(0),
        note_name: None,
        req_id: None,
    };

    let citation = locator.citation();
    let parsed = TranscriptEvidenceRef::parse_display_citation(&citation)
        .expect("archive transcript citation should be structured transcript evidence");

    assert_eq!(parsed.memory_space_id, key.memory_space_id);
    assert_eq!(parsed.channel_id, key.channel_id);
    assert_eq!(parsed.conversation_id, key.conversation_id);
    assert_eq!(parsed.turn_id, "turn-a");
    assert_eq!(parsed.message_id.as_deref(), Some("message-a"));
}
