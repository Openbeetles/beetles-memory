use bm_sdk::{
    LlmClient, LlmHttpClient, MemoryProjectionRequest, MemoryRuntime, Result, RuntimeBudgetLease,
};

use crate::{
    AdapterCommand, AdapterEnvelope, AdapterErrorKey, AdapterProjectionReport, AdapterResponse,
    AdapterSdkReport,
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
    let AdapterRuntimeServices { http, llm } = services;
    let report = match envelope.payload {
        AdapterCommand::Write(request) => {
            AdapterSdkReport::Write(Box::new(runtime.write(request)?))
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
            AdapterSdkReport::LongTermMutate(Box::new(runtime.mutate_long_term_memory(*request)?))
        }
        AdapterCommand::LongTermPolicy(request) => AdapterSdkReport::LongTermPolicy(Box::new(
            runtime.mutate_memory_governance_policy(request)?,
        )),
        AdapterCommand::TranscriptAttrWrite(request) => AdapterSdkReport::TranscriptAttrWrite(
            Box::new(runtime.record_transcript_attrs(request)?),
        ),
        AdapterCommand::Capabilities => {
            AdapterSdkReport::Capabilities(Box::new(runtime.capabilities().clone()))
        }
        AdapterCommand::Close(request) => {
            AdapterSdkReport::Close(Box::new(runtime.close(request)?))
        }
    };

    Ok(AdapterResponse::Accepted {
        request_id,
        audit_id,
        report,
    })
}
