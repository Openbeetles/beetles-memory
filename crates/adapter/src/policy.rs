pub type AdapterBudget = bm_sdk::AdapterRuntimeBudget;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdapterPolicy {
    pub auth_required: bool,
    pub idempotency_required: bool,
    pub source_required: bool,
    pub private_data_allowed: bool,
}

impl AdapterPolicy {
    pub const fn authenticated() -> Self {
        Self {
            auth_required: true,
            idempotency_required: true,
            source_required: true,
            private_data_allowed: false,
        }
    }

    pub const fn runtime_budget<'report>(
        &self,
        report: &'report bm_sdk::RuntimeBudgetReport,
    ) -> &'report AdapterBudget {
        &report.adapter_budget
    }
}
