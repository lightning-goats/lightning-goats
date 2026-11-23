//! Bounded covering selection. All selected whole proofs retain every owner.
use super::*;

pub(crate) struct Candidate {
    pub y: String,
    pub amount: u64,
    parts: BTreeMap<Uuid, u64>,
}
impl Journal {
    pub(crate) fn retained_treasury_split(
        &self,
        binding: &TreasuryBinding,
        id: Uuid,
        limits: Limits,
    ) -> Result<Option<Reservation>> {
        let issued = self.treasury_instruction(binding, id)?;
        let raw: Option<Vec<u8>> = self.connection.query_row(
            "SELECT reservation FROM treasury_split_reservations WHERE intent_id=? AND length(reservation)<=1048576",
            [id.to_string()], |r| r.get(0)).optional()?;
        let Some(raw) = raw else { return Ok(None) };
        let retained: Reservation = serde_json::from_slice(&raw)?;
        retained.plan.validate_for(&issued.intent, limits)?;
        Ok(Some(retained))
    }
    pub(crate) fn treasury_split_candidates(
        &self,
        binding: &TreasuryBinding,
        id: Uuid,
        now: u64,
    ) -> Result<Vec<Candidate>> {
        let issued = self.treasury_instruction(binding, id)?;
        binding.validate_new_invoice(&issued, now)?;
        let mut outputs = BTreeMap::new();
        for source in &issued.intent.sources {
            let held: u64 = self.connection.query_row(
                "SELECT input_sats FROM treasury_source_holds WHERE intent_id=? AND acquisition_id=?",
                params![id.to_string(),source.acquisition_id.to_string()], |r| r.get(0))?;
            ensure!(
                held == source.input_sats,
                "split source value is not reserved"
            );
            let mut stmt = self.connection.prepare(
                "SELECT o.y,o.amount FROM treasury_owned_outputs o JOIN treasury_output_parts p ON p.mint_id=o.mint_id AND p.y=o.y WHERE o.mint_id=?1 AND p.acquisition_id=?2 AND NOT EXISTS(SELECT 1 FROM treasury_proof_holds h WHERE h.mint_id=o.mint_id AND h.y=o.y) ORDER BY o.amount,o.y LIMIT ?3")?;
            let rows = stmt.query_map(
                params![
                    binding.mint_id,
                    source.acquisition_id.to_string(),
                    self.max_proofs.saturating_add(1)
                ],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, u64>(1)?)),
            )?;
            for row in rows {
                let (y, amount) = row?;
                ensure!(
                    amount > 0
                        && amount <= i64::MAX as u64
                        && cdk::nuts::PublicKey::from_hex(&y)?.to_hex() == y,
                    "invalid split candidate"
                );
                if let Some(previous) = outputs.insert(y, amount) {
                    ensure!(previous == amount, "ambiguous split output");
                }
                ensure!(
                    outputs.len() <= self.max_proofs,
                    "split candidate bound exceeded"
                );
            }
        }
        let mut candidates = Vec::new();
        for (y, amount) in outputs {
            let count: u64 = self.connection.query_row(
                "SELECT COUNT(*) FROM treasury_owned_outputs WHERE mint_id=? AND y=?",
                params![binding.mint_id, y],
                |r| r.get(0),
            )?;
            ensure!(count == 1, "ambiguous split ownership origin");
            let mut stmt = self.connection.prepare("SELECT acquisition_id,amount FROM treasury_output_parts WHERE mint_id=? AND y=? ORDER BY acquisition_id LIMIT ?")?;
            let rows = stmt.query_map(params![binding.mint_id, y, MAX_SOURCES + 1], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, u64>(1)?))
            })?;
            let mut parts = BTreeMap::new();
            let mut total = 0u64;
            for row in rows {
                let (owner, value) = row?;
                let owner = Uuid::parse_str(&owner)?;
                ensure!(
                    !owner.is_nil()
                        && value > 0
                        && parts.insert(owner, value).is_none()
                        && parts.len() <= MAX_SOURCES,
                    "invalid split source parts"
                );
                total = total
                    .checked_add(value)
                    .ok_or_else(|| anyhow::anyhow!("split attribution overflow"))?;
            }
            ensure!(total == amount, "split attribution does not conserve value");
            candidates.push(Candidate { y, amount, parts });
        }
        candidates.sort_by(|a, b| a.amount.cmp(&b.amount).then(a.y.cmp(&b.y)));
        Ok(candidates)
    }

    /// Fees are a planning hint from wallet keyset metadata. The transport must
    /// authenticate them again before execution. The final reservation rechecks
    /// all ownership and holds in its existing atomic transaction.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn select_treasury_split(
        &mut self,
        binding: &TreasuryBinding,
        id: Uuid,
        split_id: Uuid,
        fees_ppk: &BTreeMap<String, u64>,
        denominations: &[u64],
        limits: Limits,
        now: u64,
    ) -> Result<Reservation> {
        if let Some(retained) = self.retained_treasury_split(binding, id, limits)? {
            ensure!(
                retained.split_id() == split_id,
                "replacement split identity refused"
            );
            return Ok(retained);
        }
        let issued = self.treasury_instruction(binding, id)?;
        let candidates = self.treasury_split_candidates(binding, id, now)?;
        ensure!(
            fees_ppk.len() == candidates.len()
                && candidates.iter().all(|c| fees_ppk.contains_key(&c.y)),
            "split fee inventory differs from candidates"
        );
        let (chosen, fee) = covering_subset(
            &candidates,
            &issued.intent,
            fees_ppk,
            denominations,
            limits,
            32768,
        )?
        .ok_or_else(|| anyhow::anyhow!("no affordable whole-proof split allocation"))?;
        let mut ys = chosen
            .into_iter()
            .map(|i| candidates[i].y.clone())
            .collect::<Vec<_>>();
        ys.sort();
        self.reserve_treasury_split(
            binding,
            id,
            &Request {
                split_id,
                input_ys: ys,
                fee_sats: fee,
                denominations: denominations.to_vec(),
            },
            limits,
            now,
        )
    }
}

fn covering_subset(
    candidates: &[Candidate],
    intent: &lightning_goats_cashu_receipt::treasury::Intent,
    fees: &BTreeMap<String, u64>,
    denominations: &[u64],
    limits: Limits,
    work_budget: usize,
) -> Result<Option<(Vec<usize>, u64)>> {
    // Iterative DFS avoids stack depth controlled by proof inventory. Each branch
    // is bounded by whole-value/fee limits; no partial selection escapes failure.
    let mut pending = vec![(0usize, Vec::new(), BTreeMap::<Uuid, u64>::new(), 0u64, 0u64)];
    let mut visited = 0;
    while let Some((index, selected, owned, gross, raw_fee)) = pending.pop() {
        ensure!(
            visited < work_budget,
            "split selection work budget exhausted"
        );
        visited += 1;
        let fee = raw_fee
            .checked_add(999)
            .ok_or_else(|| anyhow::anyhow!("split fee overflow"))?
            / 1000;
        if !selected.is_empty()
            && intent
                .sources
                .iter()
                .all(|s| owned.get(&s.acquisition_id).copied().unwrap_or(0) >= s.input_sats)
            && Plan::build(intent, owned.clone(), fee, denominations.to_vec(), limits).is_ok()
        {
            return Ok(Some((selected, fee)));
        }
        if index == candidates.len() {
            continue;
        }
        let candidate = &candidates[index];
        pending.push((index + 1, selected.clone(), owned.clone(), gross, raw_fee));
        let Some(next_gross) = gross
            .checked_add(candidate.amount)
            .filter(|n| *n <= limits.maximum_input_sats)
        else {
            continue;
        };
        let next_fee = raw_fee
            .checked_add(
                *fees
                    .get(&candidate.y)
                    .ok_or_else(|| anyhow::anyhow!("missing candidate fee"))?,
            )
            .ok_or_else(|| anyhow::anyhow!("split fee overflow"))?;
        let rounded = next_fee
            .checked_add(999)
            .ok_or_else(|| anyhow::anyhow!("split fee overflow"))?
            / 1000;
        if rounded > limits.maximum_fee_sats || rounded > intent.maximum_fee_sats {
            continue;
        }
        let mut next_owned = owned;
        for (source, value) in &candidate.parts {
            let total = next_owned.entry(*source).or_default();
            *total = total
                .checked_add(*value)
                .ok_or_else(|| anyhow::anyhow!("split source overflow"))?;
        }
        if next_owned.len() > MAX_SOURCES {
            continue;
        }
        let mut chosen = selected;
        chosen.push(index);
        pending.push((index + 1, chosen, next_owned, next_gross, next_fee));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selection_freezes_original_proofs_and_replays_without_metadata_after_expiry() -> Result<()> {
        let mut f = super::super::tests::fixture()?;
        let fees = f
            .request
            .input_ys
            .iter()
            .map(|y| (y.clone(), 100))
            .collect();
        let retained = f.journal.select_treasury_split(
            &f.binding,
            f.issued.intent.id,
            f.request.split_id,
            &fees,
            &f.request.denominations,
            super::super::tests::limits(),
            1000,
        )?;
        assert_eq!(retained.request.input_ys, f.request.input_ys);
        assert_eq!(retained.plan.fee_sats(), 1);
        let path = f.directory.path().join("wallet.sqlite");
        drop(f.journal);
        f.journal = Journal::open(&path, f.binding.wallet_epoch, 128)?;
        let replay = f.journal.select_treasury_split(
            &f.binding,
            f.issued.intent.id,
            f.request.split_id,
            &BTreeMap::new(),
            &[],
            super::super::tests::limits(),
            2000,
        )?;
        assert!(replay == retained);
        assert!(
            f.journal
                .select_treasury_split(
                    &f.binding,
                    f.issued.intent.id,
                    Uuid::new_v4(),
                    &fees,
                    &f.request.denominations,
                    super::super::tests::limits(),
                    2000
                )
                .is_err()
        );
        Ok(())
    }
    #[test]
    fn insufficient_exposure_fee_metadata_and_available_proofs_leave_no_partial_holds() -> Result<()>
    {
        let mut f = super::super::tests::fixture()?;
        let limits = super::super::tests::limits();
        for (cap, ppk, missing) in [(127, 100, false), (128, 2000, false), (128, 100, true)] {
            let mut fees: BTreeMap<_, _> = f
                .request
                .input_ys
                .iter()
                .map(|y| (y.clone(), ppk))
                .collect();
            if missing {
                fees.remove(&f.request.input_ys[0]);
            }
            assert!(
                f.journal
                    .select_treasury_split(
                        &f.binding,
                        f.issued.intent.id,
                        f.request.split_id,
                        &fees,
                        &f.request.denominations,
                        Limits {
                            maximum_input_sats: cap,
                            ..limits
                        },
                        1000
                    )
                    .is_err()
            );
            let count: u64 = f.journal.connection.query_row(
                "SELECT COUNT(*) FROM treasury_proof_holds",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(count, 0);
        }
        f.journal.connection.execute(
            "INSERT INTO treasury_proof_holds(mint_id,y,intent_id) VALUES(?,?,?)",
            params![
                f.binding.mint_id,
                f.request.input_ys[0],
                f.issued.intent.id.to_string()
            ],
        )?;
        let fees = BTreeMap::from([(f.request.input_ys[1].clone(), 100)]);
        assert!(
            f.journal
                .select_treasury_split(
                    &f.binding,
                    f.issued.intent.id,
                    f.request.split_id,
                    &fees,
                    &f.request.denominations,
                    limits,
                    1000
                )
                .is_err()
        );
        let count: u64 = f.journal.connection.query_row(
            "SELECT COUNT(*) FROM treasury_proof_holds",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(count, 1);
        Ok(())
    }
    #[test]
    fn covering_search_matches_exhaustive_feasible_subsets_and_preserves_foreign_remainder()
    -> Result<()> {
        let (_, issued) = crate::treasury::tests::fixture();
        let source = issued.intent.sources[0].acquisition_id;
        let foreign = Uuid::from_u128(999);
        let candidates = vec![
            Candidate {
                y: "a".into(),
                amount: 64,
                parts: BTreeMap::from([(source, 64)]),
            },
            Candidate {
                y: "b".into(),
                amount: 64,
                parts: BTreeMap::from([(source, 64)]),
            },
            Candidate {
                y: "c".into(),
                amount: 128,
                parts: BTreeMap::from([(source, 101), (foreign, 27)]),
            },
        ];
        let denoms = [1, 2, 4, 8, 16, 32, 64, 128];
        for cap in [64, 127, 128, 192, 256] {
            for ppk in [0u64, 100, 1000, 2000] {
                for outputs in [1, 4, 8, 32] {
                    let limits = Limits {
                        maximum_input_sats: cap,
                        maximum_fee_sats: 1,
                        maximum_outputs: outputs,
                    };
                    let fees = candidates.iter().map(|c| (c.y.clone(), ppk)).collect();
                    let possible = (1usize..8).any(|mask| {
                        let mut owned = BTreeMap::new();
                        let mut raw = 0u64;
                        for (i, c) in candidates.iter().enumerate() {
                            if mask & (1 << i) != 0 {
                                raw += ppk;
                                for (owner, value) in &c.parts {
                                    *owned.entry(*owner).or_default() += *value;
                                }
                            }
                        }
                        Plan::build(
                            &issued.intent,
                            owned,
                            raw.div_ceil(1000),
                            denoms.to_vec(),
                            limits,
                        )
                        .is_ok()
                    });
                    let actual =
                        covering_subset(&candidates, &issued.intent, &fees, &denoms, limits, 64)?;
                    assert_eq!(actual.is_some(), possible);
                }
            }
        }
        let fees = BTreeMap::from([("c".into(), 100)]);
        let result = covering_subset(
            &candidates[2..],
            &issued.intent,
            &fees,
            &denoms,
            super::super::tests::limits(),
            8,
        )?
        .unwrap();
        assert_eq!(result, (vec![0], 1));
        let plan = Plan::build(
            &issued.intent,
            candidates[2].parts.clone(),
            1,
            denoms.to_vec(),
            super::super::tests::limits(),
        )?;
        assert_eq!(
            plan.outputs()
                .iter()
                .filter(|o| o.acquisition_id == foreign)
                .map(|o| o.amount)
                .sum::<u64>(),
            27
        );
        assert!(
            covering_subset(
                &candidates[2..],
                &issued.intent,
                &fees,
                &denoms,
                super::super::tests::limits(),
                0
            )
            .is_err()
        );
        Ok(())
    }
}
