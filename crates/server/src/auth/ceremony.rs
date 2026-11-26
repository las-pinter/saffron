//! In-memory `WebAuthn` ceremony state.
//!
//! `webauthn-rs` needs the challenge state kept between the start and finish calls.
//! Keeping it in memory rather than the database means a restart only asks the
//! browser to begin again.
//!
//! Sign-in ceremonies are started anonymously, so their limits keep a flood from
//! pushing out anyone else's: each client holds a few, all of them together are
//! bounded, and they never displace a registration.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};

use jiff::{SignedDuration, Timestamp};
use uuid::Uuid;
use webauthn_rs::prelude::{DiscoverableAuthentication, PasskeyRegistration};

use super::token::TokenHash;

/// How long a client has to finish a ceremony.
pub const TTL: SignedDuration = SignedDuration::from_mins(5);
/// Unfinished registrations kept at once; the oldest goes first.
pub const MAX_REGISTRATIONS: usize = 32;
/// Unfinished sign-ins kept at once, from all clients together.
pub const MAX_AUTHENTICATIONS: usize = 256;
/// Unfinished sign-ins one client may hold; its oldest goes first.
pub const MAX_AUTHENTICATIONS_PER_SOURCE: usize = 4;

/// A registration in progress, bound to the session that started it.
#[derive(Debug)]
pub struct Registration {
    /// The user adding a passkey.
    pub user_id: i64,
    /// The session that started the ceremony.
    pub token_hash: TokenHash,
    /// The library's challenge state.
    pub state: PasskeyRegistration,
}

#[derive(Debug)]
enum Ceremony {
    Registration(Registration),
    Authentication {
        state: DiscoverableAuthentication,
        source: String,
    },
}

impl Ceremony {
    fn is_registration(&self) -> bool {
        matches!(self, Self::Registration(_))
    }

    fn source(&self) -> Option<&str> {
        match self {
            Self::Registration(_) => None,
            Self::Authentication { source, .. } => Some(source),
        }
    }
}

#[derive(Debug)]
struct Entry {
    ceremony: Ceremony,
    created: Timestamp,
}

impl Entry {
    fn live(&self, now: Timestamp) -> bool {
        now.duration_since(self.created) < TTL
    }
}

/// Ceremonies shared by every request. Each is taken at most once.
#[derive(Debug, Clone, Default)]
pub struct CeremonyStore {
    inner: Arc<Mutex<HashMap<Uuid, Entry>>>,
}

impl CeremonyStore {
    /// Stores a registration ceremony and returns its id.
    #[must_use]
    pub fn insert_registration(&self, registration: Registration, now: Timestamp) -> Uuid {
        let mut entries = self.lock();
        entries.retain(|_, entry| entry.live(now));
        evict_beyond(&mut entries, MAX_REGISTRATIONS, |entry| {
            entry.ceremony.is_registration()
        });
        insert(&mut entries, Ceremony::Registration(registration), now)
    }

    /// Takes a live registration ceremony.
    #[must_use]
    pub fn take_registration(&self, id: Uuid, now: Timestamp) -> Option<Registration> {
        match self.take(id, now)? {
            Ceremony::Registration(registration) => Some(registration),
            Ceremony::Authentication { .. } => None,
        }
    }

    /// Stores a sign-in ceremony started by `source` and returns its id.
    #[must_use]
    pub fn insert_authentication(
        &self,
        state: DiscoverableAuthentication,
        source: &str,
        now: Timestamp,
    ) -> Uuid {
        let mut entries = self.lock();
        entries.retain(|_, entry| entry.live(now));
        evict_beyond(&mut entries, MAX_AUTHENTICATIONS_PER_SOURCE, |entry| {
            entry.ceremony.source() == Some(source)
        });
        evict_beyond(&mut entries, MAX_AUTHENTICATIONS, |entry| {
            !entry.ceremony.is_registration()
        });
        let ceremony = Ceremony::Authentication {
            state,
            source: source.to_owned(),
        };
        insert(&mut entries, ceremony, now)
    }

    /// Takes a live sign-in ceremony.
    #[must_use]
    pub fn take_authentication(
        &self,
        id: Uuid,
        now: Timestamp,
    ) -> Option<DiscoverableAuthentication> {
        match self.take(id, now)? {
            Ceremony::Authentication { state, .. } => Some(state),
            Ceremony::Registration(_) => None,
        }
    }

    fn take(&self, id: Uuid, now: Timestamp) -> Option<Ceremony> {
        let entry = self.lock().remove(&id)?;
        entry.live(now).then_some(entry.ceremony)
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<Uuid, Entry>> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    #[cfg(test)]
    fn count(&self, matches: impl Fn(&Ceremony) -> bool) -> usize {
        self.lock()
            .values()
            .filter(|entry| matches(&entry.ceremony))
            .count()
    }
}

fn insert(entries: &mut HashMap<Uuid, Entry>, ceremony: Ceremony, now: Timestamp) -> Uuid {
    let id = Uuid::new_v4();
    entries.insert(
        id,
        Entry {
            ceremony,
            created: now,
        },
    );
    id
}

/// Removes the oldest entries that match `counted` until fewer than `limit` remain,
/// making room for one more.
fn evict_beyond(
    entries: &mut HashMap<Uuid, Entry>,
    limit: usize,
    counted: impl Fn(&Entry) -> bool,
) {
    let mut matching: Vec<(Timestamp, Uuid)> = entries
        .iter()
        .filter(|(_, entry)| counted(entry))
        .map(|(id, entry)| (entry.created, *id))
        .collect();
    if matching.len() < limit {
        return;
    }
    matching.sort_unstable();
    for (_, id) in &matching[..=matching.len() - limit] {
        entries.remove(id);
    }
}

#[cfg(test)]
mod tests {
    use url::Url;
    use webauthn_rs::prelude::{Webauthn, WebauthnBuilder};

    use super::*;

    fn webauthn() -> Webauthn {
        let origin = Url::parse("https://saffron.example").expect("origin");
        WebauthnBuilder::new("saffron.example", &origin)
            .expect("builder")
            .build()
            .expect("webauthn")
    }

    fn registration() -> Registration {
        let state = webauthn()
            .start_passkey_registration(Uuid::new_v4(), "owner", "Owner", None)
            .expect("start registration")
            .1;
        Registration {
            user_id: 7,
            token_hash: [1; 32],
            state,
        }
    }

    fn sign_in(store: &CeremonyStore, source: &str, second: i64) -> Uuid {
        store.insert_authentication(authentication(), source, at(second))
    }

    fn authentication() -> DiscoverableAuthentication {
        webauthn()
            .start_discoverable_authentication()
            .expect("start authentication")
            .1
    }

    fn at(seconds: i64) -> Timestamp {
        Timestamp::from_second(1_800_000_000 + seconds).expect("timestamp")
    }

    #[test]
    fn a_registration_is_taken_once() {
        let store = CeremonyStore::default();
        let id = store.insert_registration(registration(), at(0));

        let first = store.take_registration(id, at(1));
        let second = store.take_registration(id, at(1));

        assert_eq!(
            first.map(|taken| (taken.user_id, taken.token_hash)),
            Some((7, [1; 32]))
        );
        assert!(second.is_none());
    }

    #[test]
    fn an_authentication_is_taken_once() {
        let store = CeremonyStore::default();
        let id = sign_in(&store, "client", 0);

        let first = store.take_authentication(id, at(1));
        let second = store.take_authentication(id, at(1));

        assert!(first.is_some());
        assert!(second.is_none());
    }

    #[test]
    fn ceremonies_do_not_cross_kinds() {
        let store = CeremonyStore::default();
        let id = sign_in(&store, "client", 0);

        assert!(store.take_registration(id, at(1)).is_none());
    }

    #[test]
    fn a_ceremony_lives_just_under_five_minutes() {
        let store = CeremonyStore::default();
        let early = sign_in(&store, "client", 0);
        let late = sign_in(&store, "client", 0);

        assert!(store.take_authentication(early, at(299)).is_some());
        assert!(store.take_authentication(late, at(300)).is_none());
    }

    #[test]
    fn at_most_32_registrations_are_kept_and_the_oldest_goes() {
        let store = CeremonyStore::default();
        let first = store.insert_registration(registration(), at(0));
        for second in 1..40 {
            let _ = store.insert_registration(registration(), at(second));
        }

        assert_eq!(store.count(Ceremony::is_registration), 32);
        assert!(store.take_registration(first, at(40)).is_none());
    }

    #[test]
    fn one_client_holds_at_most_four_sign_ins_and_its_oldest_goes() {
        let store = CeremonyStore::default();
        let first = sign_in(&store, "client", 0);
        for second in 1..10 {
            sign_in(&store, "client", second);
        }

        assert_eq!(
            store.count(|ceremony| ceremony.source() == Some("client")),
            4
        );
        assert!(store.take_authentication(first, at(10)).is_none());
    }

    #[test]
    fn a_flooding_client_leaves_other_sign_ins_alone() {
        let store = CeremonyStore::default();
        let other = sign_in(&store, "someone else", 0);
        for _ in 0..300 {
            sign_in(&store, "flooder", 1);
        }

        assert!(store.take_authentication(other, at(2)).is_some());
    }

    #[test]
    fn at_most_256_sign_ins_are_kept_from_all_clients() {
        let store = CeremonyStore::default();
        for client in 0..300 {
            sign_in(&store, &format!("client {client}"), client);
        }

        assert_eq!(store.count(|ceremony| !ceremony.is_registration()), 256);
    }

    #[test]
    fn sign_ins_never_push_out_a_registration() {
        let store = CeremonyStore::default();
        let registration = store.insert_registration(registration(), at(0));
        for client in 0..300 {
            sign_in(&store, &format!("client {client}"), client);
        }

        assert!(store.take_registration(registration, at(299)).is_some());
    }
}
