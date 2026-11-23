//! Bounded exact selection of whole owned proofs, including shared change.
//! Denomination splitting needs a separately journaled swap; never round a source.
use super::*;
use crate::treasury::TreasuryBinding;
use lightning_goats_cashu_receipt::treasury::MAX_SOURCES;

struct Candidate {
    y: String,
    amount: u64,
    parts: Vec<u64>,
}

impl Journal {
    /// Select and freeze whole proofs for the already reserved core allocation.
    /// Exact retained selections replay after expiry. Failure reserves no proofs;
    /// source holds remain intact for a later explicit split/recovery workflow.
    pub fn select_treasury_proofs(
        &mut self,
        binding: &TreasuryBinding,
        id: Uuid,
        now: u64,
    ) -> Result<BTreeMap<Uuid, Vec<String>>> {
        self.try_select_treasury_proofs(binding,id,now)?.ok_or_else(||anyhow::anyhow!(
            "exact whole-proof allocation unavailable; retain holds for denomination splitting"))
    }

    /// Only a valid but unavailable exact subset yields None. Storage, custody
    /// inconsistency and bounded-search exhaustion remain errors, not split grants.
    pub(crate) fn try_select_treasury_proofs(
        &mut self,
        binding: &TreasuryBinding,
        id: Uuid,
        now: u64,
    ) -> Result<Option<BTreeMap<Uuid, Vec<String>>>> {
        let issued = self.treasury_instruction(binding, id)?;
        let budget = super::treasury_budget::verified(&self.connection, &issued.intent)?;
        let previous: Option<Vec<u8>> = self.connection.query_row(
            "SELECT selection FROM treasury_proof_selections WHERE intent_id=? AND length(selection)<=1048576",
            [id.to_string()], |r| r.get(0),
        ).optional()?;
        if let Some(bytes) = previous {
            let selected = serde_json::from_slice(&bytes)?;
            self.reserve_treasury_proof_ids(binding, id, &selected, now)?;
            return Ok(Some(selected));
        }
        binding.validate_new_invoice(&issued, now)?;
        let sources = &issued.intent.sources;
        let mut outputs = BTreeMap::new();
        for source in sources {
            let held: u64 = self.connection.query_row(
                "SELECT input_sats FROM treasury_source_holds WHERE intent_id=? AND acquisition_id=?",
                params![id.to_string(), source.acquisition_id.to_string()], |r| r.get(0),
            )?;
            ensure!(held == source.input_sats, "source value was not reserved");
            let mut stmt = self.connection.prepare(
                "SELECT o.y,o.amount FROM treasury_owned_outputs o JOIN treasury_output_parts p ON p.mint_id=o.mint_id AND p.y=o.y WHERE o.mint_id=?1 AND p.acquisition_id=?2 AND NOT EXISTS(SELECT 1 FROM treasury_proof_holds h WHERE h.mint_id=o.mint_id AND h.y=o.y AND h.intent_id<>?3) AND (NOT EXISTS(SELECT 1 FROM treasury_split_results WHERE intent_id=?3) OR EXISTS(SELECT 1 FROM treasury_proof_holds h WHERE h.mint_id=o.mint_id AND h.y=o.y AND h.intent_id=?3)) ORDER BY o.amount DESC,o.y LIMIT ?4",
            )?;
            let rows = stmt.query_map(
                params![
                    binding.mint_id,
                    source.acquisition_id.to_string(),
                    id.to_string(),
                    self.max_proofs.saturating_add(1)
                ],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, u64>(1)?)),
            )?;
            for row in rows {
                let (y, amount) = row?;
                ensure!(
                    amount > 0 && cdk::nuts::PublicKey::from_hex(&y)?.to_hex() == y,
                    "invalid owned output"
                );
                if let Some(previous) = outputs.insert(y, amount) {
                    ensure!(previous == amount, "conflicting owned output value");
                }
                ensure!(
                    outputs.len() <= self.max_proofs,
                    "treasury candidate bound exceeded"
                );
            }
        }
        let indices = sources
            .iter()
            .enumerate()
            .map(|(i, s)| (s.acquisition_id.to_string(), i))
            .collect::<BTreeMap<_, _>>();
        let mut candidates = Vec::new();
        for (y, amount) in outputs {
            let mut stmt = self.connection.prepare(
                "SELECT acquisition_id,amount FROM treasury_output_parts WHERE mint_id=? AND y=? ORDER BY acquisition_id LIMIT ?",
            )?;
            let rows = stmt
                .query_map(params![binding.mint_id, y, MAX_SOURCES + 1], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, u64>(1)?))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            ensure!(
                !rows.is_empty() && rows.len() <= MAX_SOURCES,
                "invalid proof attribution count"
            );
            let mut parts = vec![0u64; sources.len()];
            let mut total = 0u64;
            let mut eligible = true;
            let mut seen = std::collections::BTreeSet::new();
            for (acquisition, value) in rows {
                ensure!(
                    value > 0 && seen.insert(acquisition.clone()),
                    "invalid duplicate proof attribution"
                );
                total = total
                    .checked_add(value)
                    .ok_or_else(|| anyhow::anyhow!("proof attribution overflow"))?;
                if let Some(&index) = indices.get(&acquisition) {
                    parts[index] = value;
                } else {
                    eligible = false;
                }
            }
            ensure!(
                total == amount,
                "proof attribution does not conserve owned value"
            );
            if eligible {
                candidates.push(Candidate { y, amount, parts });
            }
        }
        candidates.sort_by(|a, b| b.amount.cmp(&a.amount).then(a.y.cmp(&b.y)));
        let required = sources
            .iter()
            .map(|s| budget.sources[&s.acquisition_id])
            .collect::<Vec<_>>();
        let Some(chosen) = exact_subset(&candidates, &required, 32_768)? else {
            return Ok(None);
        };
        let mut selected = sources
            .iter()
            .map(|s| (s.acquisition_id, Vec::new()))
            .collect::<BTreeMap<_, _>>();
        for index in chosen {
            let candidate = &candidates[index];
            for (source, amount) in sources.iter().zip(&candidate.parts) {
                if *amount != 0 {
                    selected
                        .get_mut(&source.acquisition_id)
                        .expect("known source")
                        .push(candidate.y.clone());
                }
            }
        }
        for ys in selected.values_mut() {
            ys.sort();
        }
        // The existing transaction rechecks amounts and global proof ownership;
        // a concurrent competing reservation cannot leave a partial selection.
        self.reserve_treasury_proof_ids(binding, id, &selected, now)?;
        Ok(Some(selected))
    }
}

fn exact_subset(
    candidates: &[Candidate],
    required: &[u64],
    budget: usize,
) -> Result<Option<Vec<usize>>> {
    let mut suffix = vec![vec![0u64; required.len()]; candidates.len() + 1];
    for i in (0..candidates.len()).rev() {
        ensure!(
            candidates[i].parts.len() == required.len(),
            "selection dimension mismatch"
        );
        for (j, bound) in required.iter().enumerate() {
            // Capping availability at the required amount avoids irrelevant overflow.
            suffix[i][j] = suffix[i + 1][j]
                .saturating_add(candidates[i].parts[j])
                .min(*bound);
        }
    }
    let mut pending = vec![(0usize, required.to_vec(), Vec::new())];
    let mut visited = 0usize;
    while let Some((index, remaining, selected)) = pending.pop() {
        ensure!(
            visited < budget,
            "treasury exact-selection work budget exhausted"
        );
        visited += 1;
        if remaining.iter().all(|n| *n == 0) {
            return Ok(Some(selected));
        }
        if index == candidates.len()
            || remaining
                .iter()
                .zip(&suffix[index])
                .any(|(need, available)| need > available)
        {
            continue;
        }
        let candidate = &candidates[index];
        pending.push((index + 1, remaining.clone(), selected.clone()));
        if candidate
            .parts
            .iter()
            .zip(&remaining)
            .all(|(part, need)| part <= need)
        {
            let next = remaining
                .iter()
                .zip(&candidate.parts)
                .map(|(need, part)| need - part)
                .collect();
            let mut selected = selected;
            selected.push(index);
            pending.push((index + 1, next, selected));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_selection_matches_exhaustive_shared_source_allocations() -> Result<()> {
        let candidates = [[3, 0], [0, 3], [2, 1], [1, 2], [1, 0], [0, 1]]
            .into_iter()
            .enumerate()
            .map(|(i, parts)| Candidate {
                y: i.to_string(),
                amount: parts.iter().sum(),
                parts: parts.to_vec(),
            })
            .collect::<Vec<_>>();
        for a in 0..=8 {
            for b in 0..=8 {
                let required = [a, b];
                let possible = (0..1usize << candidates.len()).any(|mask| {
                    let mut sum = [0u64; 2];
                    for (i, c) in candidates.iter().enumerate() {
                        if mask & (1 << i) != 0 {
                            for (s, part) in sum.iter_mut().zip(&c.parts) {
                                *s += part;
                            }
                        }
                    }
                    sum == required
                });
                let actual = exact_subset(&candidates, &required, 1024)?;
                assert_eq!(actual.is_some(), possible);
                if let Some(indices) = actual {
                    let mut sum = [0u64; 2];
                    for i in indices {
                        for (s, part) in sum.iter_mut().zip(&candidates[i].parts) {
                            *s += part;
                        }
                    }
                    assert_eq!(sum, required);
                }
            }
        }
        assert!(exact_subset(&candidates, &[1, 1], 0).is_err());
        assert_eq!(exact_subset(&candidates, &[3, 3], 1024)?, Some(vec![0, 1]));
        Ok(())
    }
}
