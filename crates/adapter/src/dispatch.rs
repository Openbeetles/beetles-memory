use bm_sdk::{MemoryRuntime, Result};

use crate::{AdapterCommand, AdapterEnvelope, AdapterErrorKey, AdapterResponse, AdapterSdkReport};

pub fn dispatch_adapter_command(
    runtime: &MemoryRuntime,
    envelope: AdapterEnvelope<AdapterCommand>,
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
            return Ok(AdapterResponse::Rejected {
                request_id,
                audit_id,
                error_key: AdapterErrorKey::UnsupportedOperation,
                reason: format!(
                    "maintain requires explicit LLM and HTTP service injection: {}",
                    request.reason
                ),
            });
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
