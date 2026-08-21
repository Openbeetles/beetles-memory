use bm_sdk::{
    ErrorClass, LlmClient, LlmHttpClient, MemoryMutationExecution, MemoryProjectionRequest,
    MemoryRuntime, Result, RuntimeBudgetLease,
};

use crate::{
    AdapterCapabilityReportV2, AdapterCommand, AdapterEnvelope, AdapterErrorKey,
    AdapterMutationReceiptV1, AdapterProjectionReport, AdapterResponse, AdapterSdkReport,
    AdapterTurnFinalizeReport, ExternalAiMemoryProtocolVersion,
};

pub struct AdapterRuntimeServices<'a> {
    pub http: Option<&'a mut dyn LlmHttpClient>,
    pub llm: Option<&'a (dyn LlmClient + Send + Sync)>,
}

impl<'a> AdapterRuntimeServices<'a> {
    pub const fn none() -> Self {
        Self {
            http: None,
            llm: None,
        }
    }
}

pub fn dispatch_adapter_command(
    runtime: &MemoryRuntime,
    envelope: AdapterEnvelope<AdapterCommand>,
) -> Result<AdapterResponse<AdapterSdkReport>> {
    let lease = runtime.acquire_runtime_budget_lease()?;
    dispatch_adapter_command_with_services(
        runtime,
        &lease,
        envelope,
        AdapterRuntimeServices::none(),
    )
}

pub fn project_adapter_report(
    runtime: &MemoryRuntime,
    request: MemoryProjectionRequest,
) -> Result<AdapterProjectionReport> {
    let lease = runtime.acquire_runtime_budget_lease()?;
    runtime.execute_with_runtime_budget_lease(&lease, || {
        runtime
            .project_safe(request)
            .map(AdapterProjectionReport::from)
    })
}

pub fn dispatch_adapter_command_with_services(
    runtime: &MemoryRuntime,
    lease: &RuntimeBudgetLease,
    mut envelope: AdapterEnvelope<AdapterCommand>,
    services: AdapterRuntimeServices<'_>,
) -> Result<AdapterResponse<AdapterSdkReport>> {
    if envelope.protocol_version == ExternalAiMemoryProtocolVersion::V1
        && envelope.operation.mutation_reliability()
            != crate::AdapterMutationReliability::NotMutation
    {
        return Ok(AdapterResponse::Rejected {
            request_id: envelope.request_id,
            audit_id: envelope.audit_id,
            error_key: AdapterErrorKey::RuntimeBindingMismatch,
            reason: "mutation operations require beetle-memory.external-ai.v2".to_string(),
        });
    }
    if envelope.protocol_version == ExternalAiMemoryProtocolVersion::V2
        && envelope.operation.mutation_reliability()
            == crate::AdapterMutationReliability::DurableStoreReceipt
        && envelope
            .mutation_operation_id
            .as_deref()
            .is_none_or(|value| value.is_empty() || value != value.trim())
    {
        return Ok(AdapterResponse::Rejected {
            request_id: envelope.request_id,
            audit_id: envelope.audit_id,
            error_key: AdapterErrorKey::MutationOperationIdRequired,
            reason: "V2 durable mutation requires a non-empty canonical mutation_operation_id"
                .to_string(),
        });
    }
    if let Some(reason) = envelope.runtime_binding.mismatch_reason(runtime, lease) {
        return Ok(AdapterResponse::Rejected {
            request_id: envelope.request_id,
            audit_id: envelope.audit_id,
            error_key: AdapterErrorKey::RuntimeBindingMismatch,
            reason: reason.to_string(),
        });
    }
    if let Some(reason) = envelope
        .runtime_binding
        .source_mismatch_reason(&envelope.source)
    {
        return Ok(AdapterResponse::Rejected {
            request_id: envelope.request_id,
            audit_id: envelope.audit_id,
            error_key: AdapterErrorKey::RuntimeBindingMismatch,
            reason: reason.to_string(),
        });
    }
    envelope
        .payload
        .pin_accepted_at(runtime.config().clock.now_secs());
    runtime.execute_with_runtime_budget_lease(lease, || {
        dispatch_adapter_command_with_services_in_lease(runtime, envelope, services)
    })
}

fn dispatch_adapter_command_with_services_in_lease(
    runtime: &MemoryRuntime,
    envelope: AdapterEnvelope<AdapterCommand>,
    services: AdapterRuntimeServices<'_>,
) -> Result<AdapterResponse<AdapterSdkReport>> {
    if envelope.operation != envelope.payload.operation() {
        return Ok(AdapterResponse::Rejected {
            request_id: envelope.request_id,
            audit_id: envelope.audit_id,
            error_key: AdapterErrorKey::OperationMismatch,
            reason: "adapter envelope operation does not match payload command".to_string(),
        });
    }

    let request_id = envelope.request_id;
    let audit_id = envelope.audit_id;
    let mutation_operation_id = envelope.mutation_operation_id;
    let AdapterRuntimeServices { http, llm } = services;
    let mut committed_receipt = None;
    let report = match envelope.payload {
        AdapterCommand::Write(request) => {
            let operation_id = mutation_operation_id
                .as_deref()
                .expect("V2 durable mutation identity validated before dispatch");
            match runtime.write_operation(operation_id, request) {
                Ok(MemoryMutationExecution::Committed { report, receipt }) => {
                    committed_receipt = Some(AdapterMutationReceiptV1::from_operation_id(
                        operation_id,
                        &receipt,
                    ));
                    AdapterSdkReport::Write(Box::new(report))
                }
                Ok(MemoryMutationExecution::Replayed { receipt }) => {
                    return Ok(AdapterResponse::Replayed {
                        request_id,
                        audit_id,
                        mutation_operation_id: operation_id.to_string(),
                        receipt: AdapterMutationReceiptV1::from_operation_id(
                            operation_id,
                            &receipt,
                        ),
                    });
                }
                Ok(MemoryMutationExecution::Rejected { report }) => {
                    return Ok(AdapterResponse::Rejected {
                        request_id,
                        audit_id,
                        error_key: AdapterErrorKey::RuntimeRejected,
                        reason: report.reason,
                    });
                }
                Err(error) if error.class() == Some(ErrorClass::Conflict) => {
                    return Ok(AdapterResponse::Rejected {
                        request_id,
                        audit_id,
                        error_key: AdapterErrorKey::MutationOperationConflict,
                        reason: "mutation_operation_id is already committed for a different intent"
                            .to_string(),
                    });
                }
                Err(error) => return Err(error),
            }
        }
        AdapterCommand::FinalizeTurn(request) => {
            let turn_id = request.turn.turn_id.clone();
            let report = runtime.finalize_turn(*request)?;
            AdapterSdkReport::FinalizeTurn(Box::new(AdapterTurnFinalizeReport::from_sdk(
                turn_id, report,
            )))
        }
        AdapterCommand::Recall(request) => {
            AdapterSdkReport::Recall(Box::new(runtime.recall(request)?))
        }
        AdapterCommand::Project(request) => {
            AdapterSdkReport::Project(Box::new(project_adapter_report(runtime, request)?))
        }
        AdapterCommand::Maintain(request) => {
            let Some(http) = http else {
                return Ok(AdapterResponse::Rejected {
                    request_id,
                    audit_id,
                    error_key: AdapterErrorKey::UnsupportedOperation,
                    reason: "maintain requires an injected LLM HTTP client".to_string(),
                });
            };
            let Some(llm) = llm else {
                return Ok(AdapterResponse::Rejected {
                    request_id,
                    audit_id,
                    error_key: AdapterErrorKey::UnsupportedOperation,
                    reason: "maintain requires an injected LLM client".to_string(),
                });
            };
            AdapterSdkReport::Maintain(Box::new(runtime.maintain(http, llm, request)?))
        }
        AdapterCommand::Inspect(request) => {
            AdapterSdkReport::Inspect(Box::new(runtime.inspect(request)?))
        }
        AdapterCommand::Recover(request) => {
            AdapterSdkReport::Recover(Box::new(runtime.recover(request)?))
        }
        AdapterCommand::Replay(request) => {
            AdapterSdkReport::Replay(Box::new(runtime.replay(request)?))
        }
        AdapterCommand::LongTermList(request) => {
            AdapterSdkReport::LongTermList(Box::new(runtime.list_long_term_memory(request)?))
        }
        AdapterCommand::LongTermDetail(request) => {
            AdapterSdkReport::LongTermDetail(Box::new(runtime.get_long_term_memory(request)?))
        }
        AdapterCommand::LongTermMutate(request) => {
            let operation_id = mutation_operation_id
                .as_deref()
                .expect("V2 durable mutation identity validated before dispatch");
            match runtime.mutate_long_term_memory_operation(operation_id, *request) {
                Ok(MemoryMutationExecution::Committed { report, receipt }) => {
                    committed_receipt = Some(AdapterMutationReceiptV1::from_operation_id(
                        operation_id,
                        &receipt,
                    ));
                    AdapterSdkReport::LongTermMutate(Box::new(report))
                }
                Ok(MemoryMutationExecution::Replayed { receipt }) => {
                    return Ok(AdapterResponse::Replayed {
                        request_id,
                        audit_id,
                        mutation_operation_id: operation_id.to_string(),
                        receipt: AdapterMutationReceiptV1::from_operation_id(
                            operation_id,
                            &receipt,
                        ),
                    });
                }
                Ok(MemoryMutationExecution::Rejected { report }) => {
                    return Ok(AdapterResponse::Rejected {
                        request_id,
                        audit_id,
                        error_key: AdapterErrorKey::RuntimeRejected,
                        reason: report.reason,
                    });
                }
                Err(error) if error.class() == Some(ErrorClass::Conflict) => {
                    return Ok(AdapterResponse::Rejected {
                        request_id,
                        audit_id,
                        error_key: AdapterErrorKey::MutationOperationConflict,
                        reason: "mutation_operation_id is already committed for a different intent"
                            .to_string(),
                    });
                }
                Err(error) => return Err(error),
            }
        }
        AdapterCommand::LongTermPolicy(request) => AdapterSdkReport::LongTermPolicy(Box::new(
            runtime.mutate_memory_governance_policy(request)?,
        )),
        AdapterCommand::TranscriptAttrWrite(request) => AdapterSdkReport::TranscriptAttrWrite(
            Box::new(runtime.record_transcript_attrs(request)?),
        ),
        AdapterCommand::Capabilities => AdapterSdkReport::Capabilities(Box::new(
            AdapterCapabilityReportV2::for_runtime(runtime),
        )),
        AdapterCommand::Close(request) => {
            AdapterSdkReport::Close(Box::new(runtime.close(request)?))
        }
    };

    Ok(AdapterResponse::Accepted {
        request_id,
        audit_id,
        report,
        receipt: committed_receipt,
    })
}
