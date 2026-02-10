//! Key rotation tasks of the relay.

use std::{
    sync::Arc,
    time::{Duration, SystemTime},
};

use tor_keymgr::{KeyMgr, KeystoreSelector};
use tor_relay_crypto::pk::{RelaySigningKeypair, RelaySigningKeypairSpecifier, Timestamp};
use tor_rtcompat::{Runtime, SleepProviderExt};

/// Generate `KP_relaysign_ed` key.
///
/// TODO: this should also generate the corresponding certificate as well.
pub(crate) fn generate_relaysign(keymgr: &KeyMgr) -> Result<Timestamp, tor_keymgr::Error> {
    let mut rng = tor_llcrypto::rng::CautiousRng;
    // TODO: allow configuring expiration time
    // This value was chosen because it is the C Tor default
    let valid_until = Timestamp::from(SystemTime::now() + Duration::from_secs(30 * 86400));
    let key_spec = RelaySigningKeypairSpecifier::new(valid_until);

    match keymgr.generate::<RelaySigningKeypair>(
        &key_spec,
        KeystoreSelector::default(),
        &mut rng,
        false,
    ) {
        // Key already existing can happen due to wall clock strangeness,
        // so simply ignore it.
        Ok(_) | Err(tor_keymgr::Error::KeyAlreadyExists) => (),
        Err(e) => return Err(e),
    };
    Ok(valid_until)
}

/// Task to rotate keys when they need to be rotated.
pub(crate) async fn rotate_keys_task<R: Runtime>(
    runtime: R,
    keymgr: Arc<KeyMgr>,
) -> anyhow::Result<void::Void> {
    loop {
        let mut soonest_expiry: Option<Timestamp> = None;
        let relay_signing_keys = keymgr.list_matching(&tor_keymgr::KeyPathPattern::Arti(
            "relay/ks_relaysign_ed+*".to_string(),
        ))?;

        if relay_signing_keys.is_empty() {
            generate_relaysign(&keymgr)?;
        }

        for key in relay_signing_keys {
            let key_specifier: RelaySigningKeypairSpecifier = key.key_path().try_into()?;

            let valid_until = if key_specifier.valid_until() <= Timestamp::from(SystemTime::now()) {
                tracing::info!("Rotating KP_relaysign_ed key...");
                keymgr.remove_entry(&key)?;

                generate_relaysign(&keymgr)?
            } else {
                key_specifier.valid_until()
            };

            soonest_expiry = match soonest_expiry {
                Some(soonest_expiry) => Some(soonest_expiry.min(valid_until)),
                None => Some(valid_until),
            };
        }

        let next_wake = soonest_expiry
            .unwrap_or(Timestamp::from(SystemTime::now() + Duration::from_secs(60)))
            .into();
        runtime.sleep_until_wallclock(next_wake).await;
    }
}
