use bm_sdk::{
    resolve_memory_capabilities, MemoryCapabilityPolicy, MemoryPrivacyPolicy, ProfileId,
    RuntimeSkillRecallTransport,
};

#[test]
fn governed_state_and_benchmark_profile_participation_totals_are_exact() {
    let policy = MemoryCapabilityPolicy::strict_profile();
    let privacy = MemoryPrivacyPolicy::standard_private_boundary();
    let catalogs = [
        ProfileId::EspStandaloneMemory,
        ProfileId::EspEmbeddedSdk,
        ProfileId::LinuxDeviceStandaloneMemory,
        ProfileId::DesktopMacosStandaloneMemory,
        ProfileId::DesktopMacosEmbeddedSdk,
        ProfileId::DesktopMacosDevFull,
        ProfileId::DesktopLinuxEmbeddedSdk,
        ProfileId::DesktopWindowsEmbeddedSdk,
        ProfileId::DesktopWindowsDevFull,
        ProfileId::ServerLinuxMemoryGateway,
        ProfileId::ServerLinuxDevFull,
    ]
    .into_iter()
    .map(|profile| {
        resolve_memory_capabilities(profile, &policy, &privacy).expect("profile catalog")
    })
    .collect::<Vec<_>>();
    assert_eq!(catalogs.len(), 11);
    assert_eq!(
        catalogs
            .iter()
            .filter(|catalog| catalog.governed_state.dynamic_state_recall.profile_allowed)
            .count(),
        11
    );
    assert_eq!(
        catalogs
            .iter()
            .filter(|catalog| catalog
                .governed_state
                .historical_as_of_recall
                .profile_allowed)
            .count(),
        8
    );
    assert_eq!(
        catalogs
            .iter()
            .filter(|catalog| catalog.governed_state.procedural_recall.profile_allowed)
            .count(),
        11
    );
    assert_eq!(
        catalogs
            .iter()
            .filter(|catalog| catalog
                .governed_state
                .environment_premise_evaluation
                .profile_allowed)
            .count(),
        11
    );
    assert_eq!(
        catalogs
            .iter()
            .filter(|catalog| catalog
                .governed_state
                .update_lineage_inspection
                .profile_allowed)
            .count(),
        9
    );
    assert_eq!(
        catalogs
            .iter()
            .filter(|catalog| catalog.validation.benchmark_gate.profile_allowed)
            .count(),
        3
    );
}

#[test]
fn governed_state_profile_participation_matrix_is_typed_and_not_yet_advertised() {
    let policy = MemoryCapabilityPolicy::strict_profile();
    let privacy = MemoryPrivacyPolicy::standard_private_boundary();
    let mut historical_allowed_count = 0;
    let mut historical_blocked_count = 0;
    let expectations = [
        (ProfileId::EspStandaloneMemory, false, false, true),
        (ProfileId::EspEmbeddedSdk, false, false, true),
        (ProfileId::LinuxDeviceStandaloneMemory, false, true, true),
        (ProfileId::DesktopMacosStandaloneMemory, true, true, true),
        (ProfileId::DesktopMacosEmbeddedSdk, true, true, true),
        (ProfileId::DesktopMacosDevFull, true, true, true),
        (ProfileId::DesktopLinuxEmbeddedSdk, true, true, true),
        (ProfileId::DesktopWindowsEmbeddedSdk, true, true, true),
        (ProfileId::DesktopWindowsDevFull, true, true, true),
        (ProfileId::ServerLinuxMemoryGateway, true, true, true),
        (ProfileId::ServerLinuxDevFull, true, true, true),
    ];

    for (profile, historical_allowed, lineage_allowed, environment_premise_allowed) in expectations
    {
        let catalog = resolve_memory_capabilities(profile, &policy, &privacy).expect("catalog");
        assert!(catalog.governed_state.dynamic_state_recall.profile_allowed);
        assert_eq!(
            catalog
                .governed_state
                .historical_as_of_recall
                .profile_allowed,
            historical_allowed
        );
        if historical_allowed {
            historical_allowed_count += 1;
        } else {
            historical_blocked_count += 1;
        }
        assert!(!catalog.governed_state.historical_as_of_recall.compiled);
        assert!(!catalog.governed_state.historical_as_of_recall.visible);
        assert!(catalog.governed_state.procedural_recall.profile_allowed);
        assert_eq!(
            catalog
                .governed_state
                .environment_premise_evaluation
                .profile_allowed,
            environment_premise_allowed
        );
        assert_eq!(
            catalog
                .governed_state
                .update_lineage_inspection
                .profile_allowed,
            lineage_allowed
        );
        for operation in [
            catalog.governed_state.dynamic_state_recall,
            catalog.governed_state.historical_as_of_recall,
            catalog.governed_state.procedural_recall,
            catalog.governed_state.environment_premise_evaluation,
            catalog.governed_state.update_lineage_inspection,
        ] {
            assert!(!operation.compiled);
            assert!(!operation.visible);
        }
        assert_eq!(
            catalog.governed_state.runtime_skill_recall_transport,
            RuntimeSkillRecallTransport::Unavailable
        );
    }
    assert_eq!(historical_allowed_count, 8);
    assert_eq!(historical_blocked_count, 3);
}

#[test]
fn esp_standalone_and_embedded_sdk_have_distinct_visible_catalogs() {
    let policy = MemoryCapabilityPolicy::strict_profile();
    let privacy = MemoryPrivacyPolicy::standard_private_boundary();

    let standalone = resolve_memory_capabilities(ProfileId::EspStandaloneMemory, &policy, &privacy)
        .expect("standalone catalog");
    let embedded = resolve_memory_capabilities(ProfileId::EspEmbeddedSdk, &policy, &privacy)
        .expect("embedded catalog");

    assert_ne!(standalone.profile, embedded.profile);
    assert!(standalone.write.visible);
    assert!(standalone.recall.visible);
    assert!(standalone.projection.visible);
    assert!(standalone.lifecycle.recover.visible);
    assert!(!standalone.lifecycle.maintain_full.visible);
    assert!(standalone.lifecycle.maintain_lightweight.visible);
    assert!(standalone.lifecycle.operator_diagnosis.visible);
    assert!(!standalone.sqlite_index_recall.archive.visible);
    assert!(!standalone.communication_adapter.visible);
    assert!(standalone.adapter.cli.profile_allowed);
    assert!(!standalone.adapter.cli.visible);
    assert!(standalone.adapter.wss.client_allowed);
    assert!(!standalone.adapter.wss.server_allowed);
    assert_eq!(
        standalone.validation.compact_replay_fixture.visible,
        standalone.validation.compact_replay_fixture.compiled
    );
    assert_eq!(
        standalone.validation.memory_harness.visible,
        standalone.validation.memory_harness.compiled
    );
    assert!(!standalone.validation.full_replay_suite.visible);
    assert!(standalone.validation.compact_proposal_sandbox.visible);
    assert!(!standalone.validation.full_proposal_sandbox.visible);
    assert!(standalone.validation.proposal_submission.visible);

    assert!(embedded.write.visible);
    assert!(embedded.recall.visible);
    assert!(embedded.projection.visible);
    assert!(!embedded.maintenance.visible);
    assert!(!embedded.lifecycle.recover.visible);
    assert!(!embedded.lifecycle.maintain_full.visible);
    assert!(!embedded.lifecycle.maintain_lightweight.visible);
    assert!(embedded.lifecycle.operator_diagnosis.visible);
    assert!(embedded.transcript_replay.visible);
    assert!(!embedded.replay.visible);
    assert!(!embedded.sqlite_index_recall.archive.visible);
    assert!(!embedded.communication_adapter.visible);
    assert!(!embedded.adapter.cli.profile_allowed);
    assert!(!embedded.adapter.wss.profile_allowed);
    assert!(!embedded.validation.compact_replay_fixture.visible);
    assert!(embedded.validation.proposal_preview.visible);
    assert!(!embedded.validation.compact_proposal_sandbox.visible);
    assert!(!embedded.validation.proposal_submission.visible);
}

#[test]
fn desktop_profiles_allow_safe_transcript_replay_without_debug_replay() {
    let policy = MemoryCapabilityPolicy::strict_profile();
    let privacy = MemoryPrivacyPolicy::standard_private_boundary();

    for profile in [
        ProfileId::DesktopMacosStandaloneMemory,
        ProfileId::DesktopMacosEmbeddedSdk,
        ProfileId::DesktopLinuxEmbeddedSdk,
        ProfileId::DesktopWindowsEmbeddedSdk,
    ] {
        let catalog =
            resolve_memory_capabilities(profile, &policy, &privacy).expect("desktop catalog");
        assert!(
            catalog.transcript_replay.visible,
            "{} should allow HostUi transcript replay",
            profile.as_str()
        );
        assert!(
            !catalog.replay.visible,
            "{} should keep intelligence replay disabled",
            profile.as_str()
        );
        assert!(
            !catalog.lifecycle.replay_inspection.visible,
            "{} should keep replay inspection disabled",
            profile.as_str()
        );
    }
}

#[test]
fn desktop_embedded_profiles_expose_host_triggered_maintenance_and_transcript_export_only() {
    let policy = MemoryCapabilityPolicy::strict_profile();
    let privacy = MemoryPrivacyPolicy::standard_private_boundary();

    for profile in [
        ProfileId::DesktopMacosEmbeddedSdk,
        ProfileId::DesktopLinuxEmbeddedSdk,
        ProfileId::DesktopWindowsEmbeddedSdk,
    ] {
        let catalog =
            resolve_memory_capabilities(profile, &policy, &privacy).expect("desktop catalog");
        assert!(catalog.maintenance.visible, "{}", profile.as_str());
        assert!(
            catalog.lifecycle.maintain_full.visible,
            "{}",
            profile.as_str()
        );
        assert!(
            catalog.lifecycle.maintain_lightweight.visible,
            "{}",
            profile.as_str()
        );
        assert!(catalog.transcript_export.visible, "{}", profile.as_str());
        assert!(!catalog.export.visible, "{}", profile.as_str());
        assert!(!catalog.import.visible, "{}", profile.as_str());
        assert!(
            !catalog.lifecycle.export_snapshot.visible,
            "{}",
            profile.as_str()
        );
        assert!(
            !catalog.lifecycle.import_snapshot.visible,
            "{}",
            profile.as_str()
        );
    }

    let esp = resolve_memory_capabilities(ProfileId::EspEmbeddedSdk, &policy, &privacy)
        .expect("esp embedded catalog");
    assert!(!esp.maintenance.visible);
    assert!(!esp.lifecycle.maintain_full.visible);
    assert!(!esp.lifecycle.maintain_lightweight.visible);
    assert!(!esp.transcript_export.visible);
}

#[test]
fn long_term_control_capabilities_follow_profile_and_policy_boundaries() {
    let policy = MemoryCapabilityPolicy::strict_profile();
    let privacy = MemoryPrivacyPolicy::standard_private_boundary();

    for profile in [
        ProfileId::DesktopMacosStandaloneMemory,
        ProfileId::DesktopMacosEmbeddedSdk,
        ProfileId::DesktopLinuxEmbeddedSdk,
        ProfileId::DesktopWindowsEmbeddedSdk,
        ProfileId::ServerLinuxMemoryGateway,
        ProfileId::ServerLinuxDevFull,
    ] {
        let catalog = resolve_memory_capabilities(profile, &policy, &privacy).expect("catalog");
        assert!(
            catalog.long_term_control_inspect.visible,
            "{} should expose long-term inspect/list/detail",
            profile.as_str()
        );
        assert!(
            catalog.long_term_control_mutation.visible,
            "{} should expose governed targeted long-term mutation",
            profile.as_str()
        );
        assert!(
            catalog.long_term_control_policy.visible,
            "{} should expose pause/suppression policy control",
            profile.as_str()
        );
        assert!(
            catalog.long_term_control_bulk_forget.visible,
            "{} should expose bulk forget with confirmation",
            profile.as_str()
        );
    }

    for profile in [
        ProfileId::EspStandaloneMemory,
        ProfileId::EspEmbeddedSdk,
        ProfileId::LinuxDeviceStandaloneMemory,
    ] {
        let catalog =
            resolve_memory_capabilities(profile, &policy, &privacy).expect("compact catalog");
        assert!(catalog.long_term_control_inspect.visible);
        assert!(catalog.long_term_control_mutation.visible);
        assert!(catalog.long_term_control_policy.visible);
        assert!(
            !catalog.long_term_control_bulk_forget.visible,
            "{} should hide destructive bulk forget",
            profile.as_str()
        );
    }

    let mut disabled = MemoryCapabilityPolicy::strict_profile();
    disabled.long_term_control_mutation_enabled = false;
    disabled.long_term_control_policy_enabled = false;
    disabled.long_term_control_bulk_forget_enabled = false;
    let catalog = resolve_memory_capabilities(ProfileId::ServerLinuxDevFull, &disabled, &privacy)
        .expect("disabled catalog");
    assert!(catalog.long_term_control_inspect.visible);
    assert!(!catalog.long_term_control_mutation.visible);
    assert!(!catalog.long_term_control_policy.visible);
    assert!(!catalog.long_term_control_bulk_forget.visible);
}

#[test]
fn server_gateway_can_surface_adapter_permission_without_creating_adapter_code() {
    let mut policy = MemoryCapabilityPolicy::strict_profile();
    policy.communication_adapter_enabled = true;
    let privacy = MemoryPrivacyPolicy::standard_private_boundary();

    let catalog =
        resolve_memory_capabilities(ProfileId::ServerLinuxMemoryGateway, &policy, &privacy)
            .expect("server gateway catalog");

    assert!(catalog.communication_adapter.profile_allowed);
    assert!(catalog.communication_adapter.config_enabled);
    assert!(catalog.communication_adapter.visible);
    assert!(catalog.adapter.http.visible);
    assert!(catalog.adapter.http.server_allowed);
    assert!(catalog.adapter.wss.visible);
    assert!(catalog.adapter.mcp.visible);
    assert!(catalog.adapter.a2a.visible);
    assert!(catalog.entry.http_server.visible);
    assert!(catalog.entry.wss_server.visible);
    assert!(catalog.entry.mcp_server.visible);
    assert!(catalog.entry.a2a_bridge.visible);
    assert!(catalog.entry.llm_gateway_server.visible);
    assert_eq!(
        catalog.validation.full_replay_suite.visible,
        catalog.validation.full_replay_suite.compiled
    );
    assert!(catalog.validation.full_proposal_sandbox.visible);
    assert!(catalog.validation.proposal_submission.visible);
}

#[test]
fn entry_runtime_visibility_distinguishes_esp_deployment_roles() {
    let mut policy = MemoryCapabilityPolicy::strict_profile();
    policy.communication_adapter_enabled = true;
    let privacy = MemoryPrivacyPolicy::standard_private_boundary();

    let standalone = resolve_memory_capabilities(ProfileId::EspStandaloneMemory, &policy, &privacy)
        .expect("standalone catalog");
    let embedded = resolve_memory_capabilities(ProfileId::EspEmbeddedSdk, &policy, &privacy)
        .expect("embedded catalog");

    assert!(standalone.entry.cli.visible);
    assert!(standalone.entry.wss_client.visible);
    assert!(!standalone.entry.http_server.visible);
    assert!(!standalone.entry.mcp_server.visible);
    assert!(!standalone.entry.a2a_bridge.visible);
    assert!(!standalone.entry.llm_gateway_server.visible);

    assert!(!embedded.entry.cli.visible);
    assert!(!embedded.entry.wss_client.visible);
    assert!(!embedded.entry.http_server.visible);
    assert!(!embedded.entry.mcp_server.visible);
    assert!(!embedded.entry.a2a_bridge.visible);
    assert!(!embedded.entry.llm_gateway_server.visible);
}

#[test]
fn llm_gateway_entry_is_limited_to_server_profiles() {
    let mut policy = MemoryCapabilityPolicy::strict_profile();
    policy.communication_adapter_enabled = true;
    let privacy = MemoryPrivacyPolicy::standard_private_boundary();

    for profile in [
        ProfileId::ServerLinuxMemoryGateway,
        ProfileId::ServerLinuxDevFull,
        ProfileId::DesktopMacosStandaloneMemory,
    ] {
        let catalog =
            resolve_memory_capabilities(profile, &policy, &privacy).expect("server catalog");
        assert!(
            catalog.entry.llm_gateway_server.visible,
            "{} should expose llm gateway server",
            profile.as_str()
        );
    }

    for profile in [
        ProfileId::EspStandaloneMemory,
        ProfileId::EspEmbeddedSdk,
        ProfileId::LinuxDeviceStandaloneMemory,
        ProfileId::DesktopMacosEmbeddedSdk,
        ProfileId::DesktopLinuxEmbeddedSdk,
        ProfileId::DesktopWindowsEmbeddedSdk,
    ] {
        let catalog =
            resolve_memory_capabilities(profile, &policy, &privacy).expect("non-server catalog");
        assert!(
            !catalog.entry.llm_gateway_server.visible,
            "{} should hide llm gateway server",
            profile.as_str()
        );
    }
}

#[test]
fn privacy_gate_blocks_projection_and_export_visibility() {
    let policy = MemoryCapabilityPolicy::strict_profile();
    let privacy = MemoryPrivacyPolicy {
        prompt_projection_allowed: false,
        private_plane_projection_allowed: false,
        operator_inspection_allowed: true,
        export_allowed: false,
        import_allowed: true,
    };

    let catalog = resolve_memory_capabilities(ProfileId::ServerLinuxDevFull, &policy, &privacy)
        .expect("dev full catalog");

    assert!(!catalog.projection.visible);
    assert!(!catalog.transcript_export.visible);
    assert!(!catalog.export.visible);
    assert!(!catalog.lifecycle.export_snapshot.visible);
    assert!(catalog.import.visible);
    assert!(catalog.lifecycle.import_snapshot.visible);
}

#[test]
fn policy_can_disable_replay_harness_and_evolution_sandbox_independently() {
    let mut policy = MemoryCapabilityPolicy::strict_profile();
    policy.replay_harness_enabled = false;
    policy.evolution_sandbox_enabled = false;
    let privacy = MemoryPrivacyPolicy::standard_private_boundary();

    let catalog = resolve_memory_capabilities(ProfileId::ServerLinuxDevFull, &policy, &privacy)
        .expect("dev full catalog");

    assert!(catalog.validation.full_replay_suite.profile_allowed);
    assert!(!catalog.validation.full_replay_suite.config_enabled);
    assert!(!catalog.validation.full_replay_suite.visible);
    assert!(catalog.validation.full_proposal_sandbox.profile_allowed);
    assert!(!catalog.validation.full_proposal_sandbox.config_enabled);
    assert!(!catalog.validation.full_proposal_sandbox.visible);
    assert!(!catalog.validation.proposal_submission.visible);
}
