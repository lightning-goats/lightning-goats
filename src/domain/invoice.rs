use std::{error::Error, fmt};

use uuid::Uuid;

pub const CLNADDRESS_LABEL_PREFIX: &str = "clnaddress:v1:";
const MAX_USER_LEN: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClnAddressInvoiceLabel {
    user: String,
    invoice_id: Uuid,
}

impl ClnAddressInvoiceLabel {
    pub fn parse(label: &str) -> Result<Self, InvoiceLabelError> {
        let remainder = label
            .strip_prefix(CLNADDRESS_LABEL_PREFIX)
            .ok_or(InvoiceLabelError::WrongPrefix)?;
        let (user, invoice_id) = remainder
            .split_once(':')
            .ok_or(InvoiceLabelError::Malformed)?;

        validate_user(user)?;

        let invoice_id = Uuid::parse_str(invoice_id).map_err(|_| InvoiceLabelError::InvalidUuid)?;

        Ok(Self {
            user: user.to_owned(),
            invoice_id,
        })
    }

    #[must_use]
    pub fn user(&self) -> &str {
        &self.user
    }

    #[must_use]
    pub const fn invoice_id(&self) -> Uuid {
        self.invoice_id
    }

    #[must_use]
    pub fn is_for_user(&self, expected_user: &str) -> bool {
        self.user == expected_user
    }
}

pub fn validate_user(user: &str) -> Result<(), InvoiceLabelError> {
    if user.is_empty() || user.len() > MAX_USER_LEN {
        return Err(InvoiceLabelError::InvalidUser);
    }

    if user.bytes().all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
    }) {
        Ok(())
    } else {
        Err(InvoiceLabelError::InvalidUser)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvoiceLabelError {
    WrongPrefix,
    Malformed,
    InvalidUser,
    InvalidUuid,
}

impl fmt::Display for InvoiceLabelError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::WrongPrefix => "invoice label is not in the clnaddress v1 namespace",
            Self::Malformed => "invoice label is malformed",
            Self::InvalidUser => "invoice label contains an invalid Lightning Address user",
            Self::InvalidUuid => "invoice label contains an invalid UUID",
        };
        formatter.write_str(message)
    }
}

impl Error for InvoiceLabelError {}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "550e8400-e29b-41d4-a716-446655440000";

    #[test]
    fn parses_herd_label() {
        let label = ClnAddressInvoiceLabel::parse(&format!("clnaddress:v1:herd:{ID}"))
            .expect("valid herd label");

        assert_eq!(label.user(), "herd");
        assert!(label.is_for_user("herd"));
        assert_eq!(label.invoice_id(), Uuid::parse_str(ID).unwrap());
    }

    #[test]
    fn rejects_non_clnaddress_label() {
        assert_eq!(
            ClnAddressInvoiceLabel::parse("ordinary-invoice"),
            Err(InvoiceLabelError::WrongPrefix)
        );
    }

    #[test]
    fn rejects_extra_delimiter_in_user_or_id() {
        assert_eq!(
            ClnAddressInvoiceLabel::parse(&format!("clnaddress:v1:herd:other:{ID}")),
            Err(InvoiceLabelError::InvalidUuid)
        );
    }

    #[test]
    fn rejects_uppercase_user() {
        assert_eq!(
            ClnAddressInvoiceLabel::parse(&format!("clnaddress:v1:Herd:{ID}")),
            Err(InvoiceLabelError::InvalidUser)
        );
    }

    #[test]
    fn rejects_colon_in_user() {
        assert!(validate_user("herd:other").is_err());
    }

    #[test]
    fn distinguishes_other_addresses() {
        let label = ClnAddressInvoiceLabel::parse(&format!("clnaddress:v1:donate:{ID}"))
            .expect("valid label");

        assert!(!label.is_for_user("herd"));
    }
}
