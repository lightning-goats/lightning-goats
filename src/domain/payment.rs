#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettledPayment {
    pub source: String,
    pub source_id: String,
    pub payment_hash: Option<String>,
    pub address_user: String,
    pub credit_pool: String,
    pub amount_msat: u64,
    pub settled_at: Option<i64>,
    pub context_json: Option<String>,
}
