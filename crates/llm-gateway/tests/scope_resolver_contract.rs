use bm_llm_gateway::{
    GatewayOllamaAppScopeConfig, GatewayScopeRequest, GatewayScopeResolver,
    GatewayScopeResolverConfig, GatewayTrustedHeaders,
};

#[test]
fn scope_resolver_never_accepts_owner_from_untrusted_headers() {
    let resolver = GatewayScopeResolver::new(GatewayScopeResolverConfig {
        local_owner_id: Some("local-owner".to_string()),
        first_run_owner_id: Some("first-run-owner".to_string()),
        default_agent_id: "agent-default".to_string(),
        default_channel: "llm.gateway".to_string(),
        default_chat_id: None,
        trusted_headers: GatewayTrustedHeaders::none(),
        ollama_app: GatewayOllamaAppScopeConfig::disabled(),
    });
    let mut request = GatewayScopeRequest::default();
    request.headers.insert(
        "x-bm-owner-id".to_string(),
        "attacker-owner-from-header".to_string(),
    );

    let resolved = resolver.resolve(&request).expect("resolve scope");

    assert_eq!(resolved.entry_scope.identity.owner_id, "local-owner");
    assert!(!resolved
        .audit_safe_summary
        .contains("attacker-owner-from-header"));
}

#[test]
fn scope_resolver_owner_source_order_uses_auth_then_local_then_first_run() {
    let resolver = GatewayScopeResolver::new(GatewayScopeResolverConfig {
        local_owner_id: Some("local-owner".to_string()),
        first_run_owner_id: Some("first-run-owner".to_string()),
        default_agent_id: "agent-default".to_string(),
        default_channel: "llm.gateway".to_string(),
        default_chat_id: None,
        trusted_headers: GatewayTrustedHeaders::none(),
        ollama_app: GatewayOllamaAppScopeConfig::disabled(),
    });
    let resolved = resolver
        .resolve(&GatewayScopeRequest {
            auth_subject: Some("token-owner".to_string()),
            ..GatewayScopeRequest::default()
        })
        .expect("resolve token owner");
    assert_eq!(resolved.entry_scope.identity.owner_id, "token-owner");

    let resolver = GatewayScopeResolver::new(GatewayScopeResolverConfig {
        local_owner_id: None,
        first_run_owner_id: Some("first-run-owner".to_string()),
        default_agent_id: "agent-default".to_string(),
        default_channel: "llm.gateway".to_string(),
        default_chat_id: None,
        trusted_headers: GatewayTrustedHeaders::none(),
        ollama_app: GatewayOllamaAppScopeConfig::disabled(),
    });
    let resolved = resolver
        .resolve(&GatewayScopeRequest::default())
        .expect("resolve first-run owner");
    assert_eq!(resolved.entry_scope.identity.owner_id, "first-run-owner");
}

#[test]
fn scope_resolver_uses_trusted_agent_channel_chat_headers_and_auth_owner_first() {
    let resolver = GatewayScopeResolver::new(GatewayScopeResolverConfig {
        local_owner_id: Some("local-owner".to_string()),
        first_run_owner_id: Some("first-run-owner".to_string()),
        default_agent_id: "agent-default".to_string(),
        default_channel: "llm.gateway".to_string(),
        default_chat_id: None,
        trusted_headers: GatewayTrustedHeaders {
            agent_id: Some("x-bm-agent-id".to_string()),
            channel: Some("x-bm-channel".to_string()),
            chat_id: Some("x-bm-chat-id".to_string()),
        },
        ollama_app: GatewayOllamaAppScopeConfig::disabled(),
    });
    let mut request = GatewayScopeRequest {
        auth_subject: Some("token-owner".to_string()),
        ..GatewayScopeRequest::default()
    };
    request
        .headers
        .insert("x-bm-agent-id".to_string(), "agent-header".to_string());
    request
        .headers
        .insert("x-bm-channel".to_string(), "ide.zed".to_string());
    request
        .headers
        .insert("x-bm-chat-id".to_string(), "chat-header".to_string());

    let resolved = resolver.resolve(&request).expect("resolve scope");

    assert_eq!(resolved.entry_scope.identity.owner_id, "token-owner");
    assert_eq!(resolved.entry_scope.identity.agent_id, "agent-header");
    assert_eq!(resolved.entry_scope.scope.channel, "ide.zed");
    assert_eq!(resolved.entry_scope.scope.chat_id, "chat-header");
}

#[test]
fn scope_resolver_can_use_configured_default_chat_id_for_local_gateway_and_mcp_pairing() {
    let resolver = GatewayScopeResolver::new(GatewayScopeResolverConfig {
        local_owner_id: Some("local-owner".to_string()),
        first_run_owner_id: Some("first-run-owner".to_string()),
        default_agent_id: "agent-default".to_string(),
        default_channel: "llm.gateway".to_string(),
        default_chat_id: Some("chat-shared".to_string()),
        trusted_headers: GatewayTrustedHeaders::none(),
        ollama_app: GatewayOllamaAppScopeConfig::disabled(),
    });

    let resolved = resolver
        .resolve(&GatewayScopeRequest::default())
        .expect("resolve configured chat");

    assert_eq!(resolved.entry_scope.scope.chat_id, "chat-shared");
}

#[test]
fn scope_resolver_stable_hash_changes_by_workspace_digest_without_leaking_path() {
    let resolver = GatewayScopeResolver::new(GatewayScopeResolverConfig::default_for_local_dev());
    let left = GatewayScopeRequest {
        workspace_root_digest: Some("digest-left".to_string()),
        workspace_root_path: Some("/Users/alice/secret/project".to_string()),
        client_conversation_hint: Some("thread-1".to_string()),
        model_alias: Some("local-model".to_string()),
        ..GatewayScopeRequest::default()
    };

    let mut right = left.clone();
    right.workspace_root_digest = Some("digest-right".to_string());

    let resolved_left_a = resolver.resolve(&left).expect("resolve left a");
    let resolved_left_b = resolver.resolve(&left).expect("resolve left b");
    let resolved_right = resolver.resolve(&right).expect("resolve right");

    assert_eq!(
        resolved_left_a.entry_scope.scope.chat_id,
        resolved_left_b.entry_scope.scope.chat_id
    );
    assert_ne!(
        resolved_left_a.entry_scope.scope.chat_id,
        resolved_right.entry_scope.scope.chat_id
    );
    assert!(!resolved_left_a.audit_safe_summary.contains("/Users/alice"));
}

#[test]
fn non_ollama_app_scope_preserves_request_id_fallback() {
    let resolver = GatewayScopeResolver::new(GatewayScopeResolverConfig::default_for_local_dev());
    let base = GatewayScopeRequest {
        model_alias: Some("local-model".to_string()),
        ..GatewayScopeRequest::default()
    };
    let mut first = base.clone();
    first.request_id_hint = Some("request-a".to_string());
    let mut second = base;
    second.request_id_hint = Some("request-b".to_string());

    let resolved_first = resolver.resolve(&first).expect("resolve first");
    let resolved_second = resolver.resolve(&second).expect("resolve second");

    assert_ne!(
        resolved_first.entry_scope.scope.chat_id,
        resolved_second.entry_scope.scope.chat_id
    );
}

#[test]
fn ollama_app_scope_ignores_transient_request_id_for_internal_new_chat_requests() {
    let mut config = GatewayScopeResolverConfig::default_for_local_dev();
    config.ollama_app = GatewayOllamaAppScopeConfig {
        enabled: true,
        local_app_identity: "ollama-app".to_string(),
    };
    let resolver = GatewayScopeResolver::new(config);
    let base = GatewayScopeRequest {
        model_alias: Some("qwen3.5:0.8b".to_string()),
        ..GatewayScopeRequest::default()
    };
    let mut first = base.clone();
    first.request_id_hint = Some("request-a".to_string());
    let mut second = base;
    second.request_id_hint = Some("request-b".to_string());

    let resolved_first = resolver.resolve(&first).expect("resolve first");
    let resolved_second = resolver.resolve(&second).expect("resolve second");

    assert_eq!(
        resolved_first.entry_scope.scope.chat_id,
        resolved_second.entry_scope.scope.chat_id
    );
    assert!(resolved_first
        .audit_safe_summary
        .contains("ollama_app_hint_source=local_app_daily_bucket"));
    assert!(!resolved_first.audit_safe_summary.contains("request-a"));
}

#[test]
fn ollama_app_scope_prefers_referer_chat_path_over_daily_bucket() {
    let mut config = GatewayScopeResolverConfig::default_for_local_dev();
    config.ollama_app = GatewayOllamaAppScopeConfig {
        enabled: true,
        local_app_identity: "ollama-app".to_string(),
    };
    let resolver = GatewayScopeResolver::new(config);
    let mut request = GatewayScopeRequest {
        model_alias: Some("qwen3.5:0.8b".to_string()),
        ..GatewayScopeRequest::default()
    };
    request.headers.insert(
        "referer".to_string(),
        "app://ollama/c/chat-from-ollama-ui".to_string(),
    );
    request.request_id_hint = Some("transient-request".to_string());

    let resolved_a = resolver.resolve(&request).expect("resolve a");
    request.request_id_hint = Some("another-transient-request".to_string());
    let resolved_b = resolver.resolve(&request).expect("resolve b");

    assert_eq!(
        resolved_a.entry_scope.scope.chat_id,
        resolved_b.entry_scope.scope.chat_id
    );
    assert!(resolved_a
        .audit_safe_summary
        .contains("ollama_app_hint_source=referer_chat_path"));
    assert!(!resolved_a.audit_safe_summary.contains("transient-request"));
}
