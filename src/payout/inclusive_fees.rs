//! Fee deduction arithmetic for a frozen member's gross share.
//! This decides the next invoice amount, never authorizes execution. Integration
//! must durably retire an unexecuted quote before requesting a reduced invoice,
//! verify its exact signature/amount/hash, and preserve ambiguous submissions.

/// Legacy plans retain their serialized representation. New plans explicitly
/// opt into provider-fee deduction; changing policy never rewrites an old plan.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    FixedAllowance,
    DeductFromShare,
}
impl Mode {
    pub fn is_fixed(&self) -> bool {
        *self == Self::FixedAllowance
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Decision {
    /// The caller still needs the durable reservation and execution checks.
    Fits {
        net: u64,
        fee: u64,
        total: u64,
    },
    /// Request a different amount; never execute the preceding oversized quote.
    ReduceInvoice {
        net: u64,
    },
    Unaffordable,
}

pub fn evaluate(
    gross: u64,
    minimum_net: u64,
    maximum_net: u64,
    invoice_net: u64,
    quoted_fee: u64,
    quoted_total: u64,
) -> Result<Decision, &'static str> {
    let maximum = i64::MAX as u64 / 1000;
    if gross == 0
        || gross > i64::MAX as u64
        || minimum_net == 0
        || maximum_net < minimum_net
        || maximum_net > maximum
        || invoice_net < minimum_net
        || invoice_net > maximum_net
        || invoice_net > gross
        || invoice_net.checked_add(quoted_fee) != Some(quoted_total)
    {
        return Err("invalid whole-sat fee observation or member budget");
    }
    if quoted_fee >= gross || gross - quoted_fee < minimum_net {
        return Ok(Decision::Unaffordable);
    }
    if quoted_total <= gross {
        return Ok(Decision::Fits {
            net: invoice_net,
            fee: quoted_fee,
            total: quoted_total,
        });
    }
    // Strictly decrease: a lower subsequent fee cannot increase an already
    // requested invoice or recreate an old payment with different terms.
    Ok(Decision::ReduceInvoice {
        net: (gross - quoted_fee).min(maximum_net),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strike_fee_is_deducted_before_a_matching_invoice_can_fit() {
        assert_eq!(
            evaluate(100, 1, 100, 100, 2, 102),
            Ok(Decision::ReduceInvoice { net: 98 })
        );
        assert_eq!(
            evaluate(100, 1, 100, 98, 2, 100),
            Ok(Decision::Fits {
                net: 98,
                fee: 2,
                total: 100
            })
        );
        assert_eq!(
            evaluate(100, 1, 100, 98, 3, 101),
            Ok(Decision::ReduceInvoice { net: 97 })
        );
    }

    #[test]
    fn lower_fee_never_increases_a_previously_requested_amount() {
        assert_eq!(
            evaluate(100, 1, 100, 98, 0, 98),
            Ok(Decision::Fits {
                net: 98,
                fee: 0,
                total: 98
            })
        );
    }

    #[test]
    fn impossible_or_unaffordable_quotes_never_fit() {
        assert!(evaluate(100, 1, 100, 98, 2, 99).is_err());
        assert!(evaluate(100, 1, 100, 101, 0, 101).is_err());
        assert!(evaluate(100, 1, 100, 100, u64::MAX, 99).is_err());
        assert_eq!(
            evaluate(100, 1, 100, 1, 100, 101),
            Ok(Decision::Unaffordable)
        );
        assert_eq!(
            evaluate(100, 10, 100, 10, 91, 101),
            Ok(Decision::Unaffordable)
        );
    }

    #[test]
    fn every_small_quote_either_fits_its_gross_or_strictly_reduces() {
        for gross in 1..128 {
            for fee in 0..128 {
                for net in 1..=gross {
                    match evaluate(gross, 1, 128, net, fee, net + fee).unwrap() {
                        Decision::Fits {
                            net: n,
                            fee: f,
                            total,
                        } => {
                            assert_eq!((n, f), (net, fee));
                            assert_eq!(n + f, total);
                            assert!(total <= gross);
                        }
                        Decision::ReduceInvoice { net: n } => {
                            assert!(n > 0 && n < net && n + fee <= gross)
                        }
                        Decision::Unaffordable => assert!(fee >= gross),
                    }
                }
            }
        }
    }
}
