//! Two issuers retain separate CDK stores under one journal and writer authority.
use super::*;

fn receipt_at(progress: Progress, stage: &str) -> Result<Box<Receipt>> {
    match progress {
        Progress::Receipt(receipt) => Ok(receipt),
        Progress::Pending(_) => bail!("{stage}: pending rather than acquired"),
        Progress::Held(_) => bail!("{stage}: held rather than acquired"),
        Progress::Acquired(_) => bail!("{stage}: receipt export unavailable"),
    }
}

async fn open<'a>(
    directory: &std::path::Path,
    epoch: Uuid,
    seed: &'a [u8; 64],
    mints: &[FixtureMint; 2],
    policies: &[MintPolicy; 2],
    keys: Arc<PinnedKeys>,
    initialize: bool,
) -> Result<Vec<Coordinator<'a, FixtureMint>>> {
    let writer = Arc::new(WriterLock::acquire(directory)?);
    let journal = Arc::new(Mutex::new(Journal::open(
        &directory.join("journal.sqlite"),
        epoch,
        16,
    )?));
    let mut result = Vec::new();
    for (mint, policy) in mints.iter().zip(policies) {
        let database =
            crate::wallet::open_mint_wallet_storage(directory, policy, &writer, initialize).await?;
        let mint_url: MintUrl = policy.advertised_url.parse()?;
        let gate = OperationGate::new(mint.clone(), journal.clone(), writer.clone(), 65536)?
            .with_wallet_database(database.clone())
            .with_key_policy(policy.id.clone(), keys.clone());
        let wallet = WalletBuilder::new()
            .mint_url(mint_url.clone())
            .unit(CurrencyUnit::Sat)
            .localstore(database.clone())
            .seed(*seed)
            .client(CustomHttpClient::with_transport(
                mint_url.clone(),
                gate.clone(),
                None,
            ))
            .without_rate_limiting()
            .use_http_subscription()
            .build()?;
        result.push(Coordinator {
            wallet,
            database: database.clone(),
            gate,
            journal: journal.clone(),
            keys: keys.clone(),
            seed,
            mint_id: policy.id.clone(),
            mint_url,
            permissions: Permissions {
                admission: true,
                acquisition: true,
                receipt_export: true,
                original_swap_replay: true,
            },
            maximum_receive_fee_sats: 1,
        });
    }
    Ok(result)
}

async fn run_case(colliding_secret: bool) -> Result<()> {
    let directory = crate::test_directory()?;
    let epoch = Uuid::new_v4();
    let seed = [42; 64];
    let receiver = SecretKey::from_slice(&[1; 32])?;
    let mut mints = [FixtureMint::new(0), FixtureMint::new(0)];
    // Independent issuer keys, rather than two aliases for one mint.
    mints[1].private_keys = Arc::new(
        (0..11)
            .map(|bit| {
                (
                    Amount::from(1_u64 << bit),
                    SecretKey::from_slice(&[40 + bit as u8; 32]).unwrap(),
                )
            })
            .collect(),
    );
    let issuer_keys = Keys::new(
        mints[1]
            .private_keys
            .iter()
            .map(|(amount, key)| (*amount, key.public_key()))
            .collect(),
    );
    mints[1].keyset.id = Id::v1_from_keys(&issuer_keys);
    mints[1].keyset.keys = issuer_keys;
    let policies = ["first", "second"].map(|id| MintPolicy {
        id: id.into(),
        advertised_url: format!("https://{id}.example"),
        policy_version: "multi-v1".into(),
        receive_cashu: true,
        receive_nutzap: false,
        bolt11_exit_qualified: true,
    });
    let registry = MintRegistry::new(policies.to_vec())?;
    let mut keys = PinnedKeys::default();
    for (mint, policy) in mints.iter().zip(&policies) {
        for (amount, key) in mint.keyset.keys.iter() {
            keys.insert(&policy.id, mint.keyset.id, u64::from(*amount), *key)?;
        }
    }
    let keys = Arc::new(keys);
    let mut inputs = Vec::new();
    for (mint, policy) in mints.iter().zip(&policies) {
        inputs.push(ordinary_json(
            &serde_json::to_vec(&json!({"id":"reusable", "mint":policy.advertised_url,
                "unit":"sat", "proofs":[mint.issue(if colliding_secret { "same-secret-distinct-issuer".to_owned() } else { format!("unique-secret-{}", policy.id) })]}))?,
            &format!("multi-mint-http-{}", policy.id),
            &["reusable".into()],
            &registry,
            &keys,
            &Limits {
                body_bytes: 65536,
                proof_count: 16,
                receipt_sats: 10000,
            },
            receiver.public_key(),
        )?);
    }
    mints[0].lose_reply.store(true, Ordering::SeqCst);
    let ids = {
        let mut coordinators = open(
            directory.path(),
            epoch,
            &seed,
            &mints,
            &policies,
            keys.clone(),
            true,
        )
        .await?;
        let first = new_id(coordinators[0].admit(&inputs[0])?);
        assert!(matches!(
            coordinators[0].acquire_new(first, &receiver).await?,
            Progress::Pending(_)
        ));
        let second = new_id(coordinators[1].admit(&inputs[1])?);
        assert_ne!(first, second);
        let acquired = receipt_at(
            coordinators[1].acquire_new(second, &receiver).await?,
            "second mint acquisition",
        )?;
        assert_eq!(acquired.net_sats, 8);
        assert_eq!(acquired.mint_id, "second");
        assert_eq!(
            coordinators[0].admit(&inputs[0])?,
            Admission::Duplicate(first)
        );
        assert_eq!(
            coordinators[1].admit(&inputs[1])?,
            Admission::Duplicate(second)
        );
        [first, second]
    };
    mints[0].cache_enabled.store(false, Ordering::SeqCst);
    let mut coordinators = open(
        directory.path(),
        epoch,
        &seed,
        &mints,
        &policies,
        keys,
        false,
    )
    .await?;
    for (index, coordinator) in coordinators.iter_mut().enumerate() {
        coordinator.permissions.admission = false;
        coordinator.permissions.acquisition = false;
        let recovered = receipt_at(
            coordinator.recover_original(ids[index]).await?,
            &format!("mint {index} recovery"),
        )?;
        assert_eq!(recovered.net_sats, 8);
        assert_eq!(recovered.mint_id, policies[index].id);
        assert_eq!(recovered.acquisition_id, ids[index]);
        assert_eq!(mints[index].executions.load(Ordering::SeqCst), 1);
    }
    Ok(())
}

#[tokio::test]
async fn one_journal_keeps_second_mint_progress_and_restores_first_without_reacquisition()
-> Result<()> {
    run_case(true).await
}

#[tokio::test]
async fn distinct_secret_control_recovers_two_mints_in_one_store() -> Result<()> {
    run_case(false).await
}
