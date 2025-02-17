use blake2::{Blake2b512, Digest};
use derive_getters::Getters;
use serde::{Deserialize, Serialize};
use serde_big_array::BigArray;
use std::fmt::Debug;
use thiserror::Error;

use crate::Timestamp;

// smth like H256 ??? (some hash type)
#[derive(Serialize, Hash, Clone)]
pub struct Hash {
    #[serde(with = "BigArray")]
    inner: [u8; 64],
    #[serde(skip)]
    compact: [u8; 4],
}

impl std::fmt::Display for Hash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:X?}", self.inner)
    }
}

impl std::fmt::Debug for Hash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Hash")
            .field("hex_value", &format!("{self}"))
            .finish()
    }
}

impl std::ops::BitXor for &Hash {
    type Output = Hash;

    fn bitxor(self, rhs: Self) -> Self::Output {
        let mut result = [0u8; 64];
        for (i, (b1, b2)) in self.inner.iter().zip(rhs.inner.iter()).enumerate() {
            result[i] = b1 ^ b2;
        }
        Hash::from_array(result)
    }
}

impl std::ops::BitXor<&Hash> for Hash {
    type Output = Hash;

    fn bitxor(mut self, rhs: &Self) -> Self::Output {
        for i in 0..self.inner.len() {
            self.inner[i] ^= rhs.inner[i];
        }
        self
    }
}

impl PartialOrd for Hash {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Hash {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.inner.cmp(&other.inner)
    }
}

impl PartialEq for Hash {
    fn eq(&self, other: &Self) -> bool {
        self.inner == other.inner
    }
}

impl Eq for Hash {}

impl Hash {
    pub fn into_array(self) -> [u8; 64] {
        return self.inner;
    }

    pub fn as_ref(&self) -> &[u8; 64] {
        return &self.inner;
    }

    pub fn as_compact(&self) -> &[u8; 4] {
        return &self.compact;
    }

    fn xor_bytes(slice: &[u8]) -> u8 {
        let mut result = 0u8;
        for b in slice {
            result ^= b;
        }
        result
    }

    fn calc_compact(inner: &[u8; 64]) -> [u8; 4] {
        let (a, c) = inner.split_at(32);
        let (a, b) = a.split_at(16);
        let (c, d) = c.split_at(16);
        let [a, b, c, d] = [a, b, c, d].map(Self::xor_bytes);
        return [a, b, c, d];
    }

    pub fn from_array(inner: [u8; 64]) -> Self {
        let compact = Self::calc_compact(&inner);
        return Hash { inner, compact };
    }
}

impl<'de> Deserialize<'de> for Hash {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = <[u8; 64]>::deserialize(deserializer)?;
        Ok(Self::from_array(s))
    }
}

#[derive(Serialize, Deserialize, Eq, PartialEq, Ord, PartialOrd, Hash, Clone, Debug)]
pub struct Signature(pub Hash);

impl std::ops::BitXor for &Signature {
    type Output = Signature;

    fn bitxor(self, rhs: Self) -> Self::Output {
        Signature(&self.0 ^ &rhs.0)
    }
}

impl std::ops::BitXor<&Signature> for Signature {
    type Output = Signature;

    fn bitxor(self, rhs: &Self) -> Self::Output {
        Signature(self.0 ^ &rhs.0)
    }
}

/// Event with unsigned metadata for navigation.
#[derive(Eq, PartialEq, Hash, Clone, Debug)]
pub struct EventWrapper<TPayload, TGenesisPayload, TPeerId> {
    // parents are inside `type_specific`, as geneses do not have ones
    pub children: Children,
    inner: SignedEvent<TPayload, TGenesisPayload, TPeerId>,
}

impl<TPayload, TGenesisPayload, TPeerId> EventWrapper<TPayload, TGenesisPayload, TPeerId> {
    pub fn new(inner: SignedEvent<TPayload, TGenesisPayload, TPeerId>) -> Self {
        EventWrapper {
            children: Children {
                self_child: SelfChild::HonestParent(None),
                other_children: vec![],
            },
            inner,
        }
    }

    pub fn inner(&self) -> &SignedEvent<TPayload, TGenesisPayload, TPeerId> {
        &self.inner
    }

    /// Event with signature that is just its hash. Does not involve any actual
    /// signing process.
    ///
    /// **Use just for testing.**
    #[cfg(test)]
    pub fn new_fakely_signed(
        payload: TPayload,
        event_kind: Kind<TGenesisPayload>,
        author: TPeerId,
        timestamp: Timestamp,
    ) -> Result<Self, bincode::Error>
    where
        TPayload: Serialize,
        TGenesisPayload: Serialize,
        TPeerId: Serialize,
    {
        let unsigned_event =
            SignedEvent::new_fakely_signed(payload, event_kind, author, timestamp)?;
        Ok(Self::new(unsigned_event))
    }
}

impl<TPayload, TGenesisPayload, TPeerId> EventWrapper<TPayload, TGenesisPayload, TPeerId> {
    pub fn hash(&self) -> &Hash {
        self.inner.hash()
    }

    pub fn signature(&self) -> &Signature {
        self.inner.signature()
    }

    pub fn kind(&self) -> &Kind<TGenesisPayload> {
        &self.inner.unsigned.fields.kind
    }

    pub fn payload(&self) -> &TPayload {
        &self.inner.unsigned.fields.user_payload
    }

    pub fn author(&self) -> &TPeerId {
        &self.inner.unsigned.fields.author
    }

    pub fn timestamp(&self) -> &u128 {
        &self.inner.unsigned.fields.timestamp
    }
}

#[derive(Eq, PartialEq, Hash, Clone, Debug, Serialize, Deserialize)]
pub struct SignedEvent<TPayload, TGenesisPayload, TPeerId> {
    unsigned: UnsignedEvent<TPayload, TGenesisPayload, TPeerId>,
    /// Hash of the fields of the event, signed by author's private key
    signature: Signature,
}

#[derive(Debug, Error)]
pub enum WithSignatureCreationError {
    #[error(transparent)]
    DigestError(#[from] bincode::Error),
    #[error("Signature provided does not match event contents and author")]
    InvalidSignature,
}

impl<TPayload, TGenesisPayload, TPeerId> SignedEvent<TPayload, TGenesisPayload, TPeerId> {
    pub fn hash(&self) -> &Hash {
        &self.unsigned.hash
    }

    pub fn signature(&self) -> &Signature {
        &self.signature
    }

    pub fn unsigned(&self) -> &UnsignedEvent<TPayload, TGenesisPayload, TPeerId> {
        &self.unsigned
    }

    pub fn into_parts(self) -> (UnsignedEvent<TPayload, TGenesisPayload, TPeerId>, Signature) {
        (self.unsigned, self.signature)
    }
}

impl<TPayload, TGenesisPayload, TPeerId> SignedEvent<TPayload, TGenesisPayload, TPeerId>
where
    TPayload: Serialize,
    TGenesisPayload: Serialize,
    TPeerId: Serialize,
{
    pub fn new<F>(
        payload: TPayload,
        event_kind: Kind<TGenesisPayload>,
        author: TPeerId,
        timestamp: Timestamp,
        sign: F,
    ) -> bincode::Result<Self>
    where
        F: FnOnce(&Hash) -> Signature,
    {
        let fields = EventFields {
            user_payload: payload,
            kind: event_kind,
            author,
            timestamp,
        };
        let unsigned_event = UnsignedEvent::new(fields)?;
        let signature = sign(&unsigned_event.hash);
        Ok(SignedEvent {
            unsigned: unsigned_event,
            signature,
        })
    }

    pub fn with_signature<F>(
        unsigned_event: UnsignedEvent<TPayload, TGenesisPayload, TPeerId>,
        signature: Signature,
        verify_signature: F,
    ) -> Result<Self, WithSignatureCreationError>
    where
        F: FnOnce(&Hash, &Signature, &TPeerId) -> bool,
    {
        let hash = unsigned_event.hash.clone();
        if verify_signature(&hash, &signature, &unsigned_event.fields.author) {
            Ok(SignedEvent {
                unsigned: unsigned_event,
                signature,
            })
        } else {
            Err(WithSignatureCreationError::InvalidSignature)
        }
    }

    #[cfg(test)]
    pub fn new_fakely_signed(
        payload: TPayload,
        event_kind: Kind<TGenesisPayload>,
        author: TPeerId,
        timestamp: Timestamp,
    ) -> Result<Self, bincode::Error>
    where
        TPayload: Serialize,
        TGenesisPayload: Serialize,
    {
        Self::new(payload, event_kind, author, timestamp, |h| {
            Signature(h.clone())
        })
    }
}

#[derive(Serialize, Deserialize, Eq, PartialEq, Hash, Clone, Debug, Getters)]
pub struct UnsignedEvent<TPayload, TGenesisPayload, TPeerId> {
    fields: EventFields<TPayload, TGenesisPayload, TPeerId>,
    hash: Hash,
}

impl<TPayload, TGenesisPayload, TPeerId> UnsignedEvent<TPayload, TGenesisPayload, TPeerId>
where
    TPayload: Serialize,
    TGenesisPayload: Serialize,
    TPeerId: Serialize,
{
    pub fn new(fields: EventFields<TPayload, TGenesisPayload, TPeerId>) -> bincode::Result<Self> {
        let mut hasher = Blake2b512::new();
        hasher.update(fields.digest()?);
        let hash_slice = &hasher.finalize()[..];
        let hash_arr: [u8; 64] = hash_slice.try_into().expect("event hashing failure");
        Ok(Self {
            fields,
            hash: Hash::from_array(hash_arr),
        })
    }
}

impl<TPayload, TGenesisPayload, TPeerId> UnsignedEvent<TPayload, TGenesisPayload, TPeerId>
where
    TPayload: Debug,
    TGenesisPayload: Debug,
    TPeerId: Debug,
{
    pub fn compact_fmt(&self) -> String {
        let kind_string = match &self.fields.kind {
            Kind::Genesis(p) => format!("Genesis({:?})", p),
            Kind::Regular(parents) => format!(
                "Regular(Parents{{ self_parent: {:?}, other_parent: {:?} }})",
                parents.self_parent.as_compact(),
                parents.other_parent.as_compact()
            ),
        };
        format!(
            "UnsignedEvent {{ user_payload: {:?}, kind: {}, author: {:?}, timestamp; {:?}, hash: {:?} }}",
            self.fields.user_payload, kind_string, self.fields.author, self.fields.timestamp, self.hash.as_compact()
        ).to_string()
    }
}

#[derive(Serialize, Deserialize, Eq, PartialEq, Hash, Clone, Debug, Getters)]
pub struct EventFields<TPayload, TGenesisPayload, TPeerId> {
    user_payload: TPayload,
    kind: Kind<TGenesisPayload>,
    author: TPeerId,
    /// Timestamp set by author
    timestamp: Timestamp,
}

impl<TPayload, TGenesisPayload, TPeerId> EventFields<TPayload, TGenesisPayload, TPeerId>
where
    TPayload: Serialize,
    TGenesisPayload: Serialize,
    TPeerId: Serialize,
{
    fn digest(&self) -> bincode::Result<Vec<u8>> {
        let mut v = vec![];
        let payload_bytes = bincode::serialize(&self.user_payload)?;
        v.extend(payload_bytes);
        let kind_bytes = bincode::serialize(&self.kind)?;
        v.extend(kind_bytes);
        let author_bytes = bincode::serialize(&self.author)?;
        v.extend(author_bytes);
        let timestamp_bytes = bincode::serialize(&self.timestamp)?;
        v.extend(timestamp_bytes);
        Ok(v)
    }
}

#[derive(Serialize, Deserialize, Eq, PartialEq, Hash, Clone, Debug)]
pub struct Children {
    // Child(-ren in case of forks) with the same author
    pub self_child: SelfChild,
    // Children  created by different peers
    pub other_children: Vec<Hash>,
}

impl Into<Vec<Hash>> for Children {
    fn into(self) -> Vec<Hash> {
        let mut result: Vec<_> = self.self_child.into();
        result.extend(self.other_children);
        result
    }
}

#[derive(Serialize, Deserialize, Eq, PartialEq, Hash, Clone, Debug)]
pub enum SelfChild {
    HonestParent(Option<Hash>),
    ForkingParent(Vec<Hash>),
}

impl SelfChild {
    /// Returns `true` if the parent became dishonest/forking
    pub fn add_child(&mut self, child: Hash) -> bool {
        // guilty until proven innocent lol :)
        let mut dishonesty = true;
        match self {
            SelfChild::HonestParent(self_child_entry) => {
                let new_val = match self_child_entry {
                    None => {
                        dishonesty = false;
                        Self::HonestParent(Some(child))
                    }
                    Some(child_2) => Self::ForkingParent(vec![child, child_2.clone()]),
                };
                *self = new_val;
            }
            SelfChild::ForkingParent(children) => children.push(child),
        };
        dishonesty
    }

    /// Returns `true` if the child was removed (thus was present before removal)
    pub fn with_child_removed(self, child: &Hash) -> Self {
        let self_children_vec: Vec<_> = self.into();
        self_children_vec
            .into_iter()
            .filter(|h| h != child)
            .collect::<Vec<_>>()
            .into()
    }
}

impl Into<Vec<Hash>> for SelfChild {
    fn into(self) -> Vec<Hash> {
        match self {
            SelfChild::HonestParent(child_opt) => child_opt.into_iter().collect(),
            SelfChild::ForkingParent(children_list) => children_list,
        }
    }
}

impl From<Vec<Hash>> for SelfChild {
    fn from(value: Vec<Hash>) -> Self {
        match &value[..] {
            [] => Self::HonestParent(None),
            [_] => Self::HonestParent(Some(value.into_iter().next().unwrap())),
            _ => Self::ForkingParent(value),
        }
    }
}

#[derive(Serialize, Deserialize, Eq, PartialEq, Hash, Clone, Debug)]
pub struct Parents {
    pub self_parent: Hash,
    pub other_parent: Hash,
}

#[derive(Serialize, Deserialize, Eq, PartialEq, Hash, Clone, Debug)]
pub enum Kind<TGenesisPayload> {
    Genesis(TGenesisPayload),
    Regular(Parents),
}

impl<G> Into<Vec<Hash>> for Kind<G> {
    fn into(self) -> Vec<Hash> {
        match self {
            Kind::Genesis(_) => vec![],
            Kind::Regular(Parents {
                self_parent,
                other_parent,
            }) => vec![self_parent, other_parent],
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use hex_literal::hex;

    use super::*;

    fn create_events() -> Result<Vec<EventWrapper<i32, (), u64>>, bincode::Error> {
        let mock_parents_1 = Parents {
            self_parent: Hash::from_array(hex!(
                "021ced8799296ceca557832ab941a50b4a11f83478cf141f51f933f653ab9fbc
                c05a037cddbed06e309bf334942c4e58cdf1a46e237911ccd7fcf9787cbc7fd0"
            )),
            other_parent: Hash::from_array(hex!(
                "a231788464c1d56aab39b098359eb00e2fd12622d85821d8bffe68fdb3044f24
                370e750986e6e4747f6ec0e051ae3e7d2558f7c4d3c4d5ab57362e572abecb36"
            )),
        };
        let mock_parents_2 = Parents {
            self_parent: Hash::from_array(hex!(
                "8a64b55fcfa60235edf16cebbfb36364d6481c3c5ec4de987114ed86c8f252c22
                3fadfa820edd589d9c723f032fdf6c9ca95f2fd95c4ffc01808812d8c1bafea"
            )),
            other_parent: Hash::from_array(hex!(
                "c3ea7982719e7197c63842e41427f358a747e96c7a849b28604569ea101b0bdc5
                6cba63e4a60b95cb29bce01c2e7e3f918d60fa35aa90586770dfc699da0361a"
            )),
        };
        let results = vec![
            EventWrapper::new_fakely_signed(0, Kind::Genesis(()), 0, 0)?,
            EventWrapper::new_fakely_signed(0, Kind::Genesis(()), 1, 0)?,
            EventWrapper::new_fakely_signed(0, Kind::Regular(mock_parents_1.clone()), 0, 0)?,
            EventWrapper::new_fakely_signed(0, Kind::Regular(mock_parents_2.clone()), 0, 0)?,
            EventWrapper::new_fakely_signed(
                0,
                Kind::Regular(Parents {
                    self_parent: mock_parents_1.self_parent.clone(),
                    other_parent: mock_parents_2.other_parent.clone(),
                }),
                0,
                0,
            )?,
            EventWrapper::new_fakely_signed(
                0,
                Kind::Regular(Parents {
                    self_parent: mock_parents_2.self_parent.clone(),
                    other_parent: mock_parents_1.other_parent.clone(),
                }),
                0,
                0,
            )?,
            EventWrapper::new_fakely_signed(1234567, Kind::Genesis(()), 0, 0)?,
            EventWrapper::new_fakely_signed(1234567, Kind::Regular(mock_parents_1.clone()), 0, 1)?,
        ];
        Ok(results)
    }

    #[test]
    fn events_create() {
        create_events().unwrap();
        // also test on various payloads
        EventWrapper::new_fakely_signed((), Kind::Genesis(()), 0, 0).unwrap();
        EventWrapper::new_fakely_signed((0,), Kind::Genesis(()), 0, 0).unwrap();
        EventWrapper::new_fakely_signed(vec![()], Kind::Genesis(()), 0, 0).unwrap();
        EventWrapper::new_fakely_signed("asdassa", Kind::Genesis(()), 0, 0).unwrap();
        EventWrapper::new_fakely_signed("asdassa".to_owned(), Kind::Genesis(()), 0, 0).unwrap();
    }

    #[test]
    fn hashes_unique() {
        let events = create_events().unwrap();
        let mut identifiers = HashSet::with_capacity(events.len());
        for n in events {
            assert!(!identifiers.contains(n.hash()));
            identifiers.insert(n.hash().clone());
        }
    }

    #[test]
    fn hash_operations_work() {
        let hash1 = Hash::from_array(hex!(
            "8a64b55fcfa60235edf16cebbfb36364d6481c3c5ec4de987114ed86c8f252c22
            3fadfa820edd589d9c723f032fdf6c9ca95f2fd95c4ffc01808812d8c1bafea"
        ));
        let hash2 = Hash::from_array(hex!(
            "c3ea7982719e7197c63842e41427f358a747e96c7a849b28604569ea101b0bdc5
            6cba63e4a60b95cb29bce01c2e7e3f918d60fa35aa90586770dfc699da0361a"
        ));
        let expected_xor = Hash::from_array(hex!(
            "498eccddbe3873a22bc92e0fab94903c710ff550244045b01151846cd8e9591e7
            53179966a8d6cd56b5cedf1f01a1530d243fd5ecf6dfa466f057d4411bb99f0"
        ));

        let xor = &hash1 ^ &hash2;
        assert_eq!(expected_xor, xor);
        let xor = &hash2 ^ &hash1;
        assert_eq!(expected_xor, xor);
        let xor = hash2 ^ &hash1;
        assert_eq!(expected_xor, xor);
    }

    #[test]
    fn hash_serializes() {
        let hash1 = Hash::from_array(hex!(
            "8a64b55fcfa60235edf16cebbfb36364d6481c3c5ec4de987114ed86c8f252c22
            3fadfa820edd589d9c723f032fdf6c9ca95f2fd95c4ffc01808812d8c1bafea"
        ));
        let hash2 = Hash::from_array(hex!(
            "c3ea7982719e7197c63842e41427f358a747e96c7a849b28604569ea101b0bdc5
            6cba63e4a60b95cb29bce01c2e7e3f918d60fa35aa90586770dfc699da0361a"
        ));

        let hash1_serialized = bincode::serialize(&hash1).unwrap();
        let hash2_serialized = bincode::serialize(&hash2).unwrap();

        let hash1_deserialized = bincode::deserialize::<Hash>(&hash1_serialized).unwrap();
        let hash2_deserialized = bincode::deserialize::<Hash>(&hash2_serialized).unwrap();

        assert_eq!(hash1, hash1_deserialized);
        assert_eq!(hash2, hash2_deserialized);

        assert_eq!(hash1.as_compact(), hash1_deserialized.as_compact());
        assert_eq!(hash2.as_compact(), hash2_deserialized.as_compact());
    }
}
