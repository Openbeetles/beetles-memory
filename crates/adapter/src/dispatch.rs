use bm_sdk::{LlmClient, LlmHttpClient, MemoryRuntime, Result};

use crate::{AdapterCommand, AdapterEnvelope, AdapterErrorKey, AdapterResponse, AdapterSdkReport};

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
    dispatch_adapter_command_with_services(runtime, envelope, AdapterRuntimeServices::none())
}

pub fn dispatch_adapter_command_with_services(
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
            AdapterSdkReport::Project(Box::new(runtime.project(request)?))
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
        AdapterCommand::Export(request) => {
            AdapterSdkReport::Export(Box::new(runtime.export(request)?))
        }
        AdapterCommand::Import(request) => {
            AdapterSdkReport::Import(Box::new(runtime.import(*request)?))
        }
        AdapterCommand::LongTermList(request) => {
            AdapterSdkReport::LongTermList(Box::new(runtime.list_long_term_memory(request)?))
        }
        AdapterCommand::LongTermDetail(request) => {
            AdapterSdkReport::LongTermDetail(Box::new(runtime.get_long_term_memory(request)?))
        }
        AdapterCommand::LongTermMutate(request) => {
            AdapterSdkReport::LongTermMutate(Box::new(runtime.mutate_long_term_memory(request)?))
        }
        AdapterCommand::LongTermPolicy(request) => AdapterSdkReport::LongTermPolicy(Box::new(
            runtime.mutate_memory_governance_policy(request)?,
        )),
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
