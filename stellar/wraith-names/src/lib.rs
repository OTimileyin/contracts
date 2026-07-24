#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, Bytes, BytesN, Env,
    String,
};

mod metadata;
use metadata::MetadataEntry;

/// Storage keys.
#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// Maps name hash (BytesN<32>) to NameEntry.
    Name(BytesN<32>),
    /// Reverse lookup: meta-address hash (BytesN<32>) to name hash (BytesN<32>).
    Reverse(BytesN<32>),
    /// Optional metadata (text records + content hash) keyed by name hash.
    Metadata(BytesN<32>),
}

/// A registered name entry.
#[contracttype]
#[derive(Clone)]
pub struct NameEntry {
    /// The human-readable name.
    pub name: String,
    /// The 64-byte stealth meta-address (spending_pubkey || viewing_pubkey).
    pub stealth_meta_address: Bytes,
    /// The registrant address (for auth).
    pub owner: Address,
}

/// Errors.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum NamesError {
    NameTaken = 1,
    NameTooShort = 2,
    NameTooLong = 3,
    InvalidNameCharacter = 4,
    InvalidMetaAddress = 5,
    NameNotFound = 6,
    NotOwner = 7,
    MetadataKeyTooLong = 50,
    MetadataValueTooLong = 51,
    MetadataRecordTooLong = 52,
    MetadataTotalTooLong = 53,
    MetadataNotFound = 54,
}

#[contract]
pub struct WraithNamesContract;

#[contractimpl]
impl WraithNamesContract {
    /// Register a name mapped to a stealth meta-address.
    /// The caller (owner) must authorize. Ownership is tied to the caller's address.
    ///
    /// # Arguments
    /// * `owner` - The address registering the name (must authorize).
    /// * `name` - The human-readable name (lowercase alphanumeric, 3-32 chars).
    /// * `stealth_meta_address` - 64-byte stealth meta-address.
    pub fn register(
        env: Env,
        owner: Address,
        name: String,
        stealth_meta_address: Bytes,
    ) -> Result<(), NamesError> {
        owner.require_auth();

        Self::validate_name(&env, &name)?;
        if stealth_meta_address.len() != 64 {
            return Err(NamesError::InvalidMetaAddress);
        }

        let name_hash = Self::hash_name(&env, &name);
        let name_key = DataKey::Name(name_hash.clone());

        // Check not taken
        if env.storage().instance().has(&name_key) {
            return Err(NamesError::NameTaken);
        }

        let entry = NameEntry {
            name: name.clone(),
            stealth_meta_address: stealth_meta_address.clone(),
            owner: owner.clone(),
        };

        env.storage().instance().set(&name_key, &entry);

        // Reverse lookup
        let meta_hash =
            BytesN::from_array(&env, &env.crypto().sha256(&stealth_meta_address).to_array());
        env.storage()
            .instance()
            .set(&DataKey::Reverse(meta_hash), &name_hash);

        env.events().publish(
            (symbol_short!("register"), name_hash),
            (name, stealth_meta_address),
        );

        Ok(())
    }

    /// Update the meta-address for an existing name.
    /// Only the current owner can update.
    pub fn update(
        env: Env,
        owner: Address,
        name: String,
        new_meta_address: Bytes,
    ) -> Result<(), NamesError> {
        owner.require_auth();

        if new_meta_address.len() != 64 {
            return Err(NamesError::InvalidMetaAddress);
        }

        let name_hash = Self::hash_name(&env, &name);
        let name_key = DataKey::Name(name_hash.clone());

        let entry: NameEntry = env
            .storage()
            .instance()
            .get(&name_key)
            .ok_or(NamesError::NameNotFound)?;

        if entry.owner != owner {
            return Err(NamesError::NotOwner);
        }

        // Remove old reverse
        let old_meta_hash = BytesN::from_array(
            &env,
            &env.crypto().sha256(&entry.stealth_meta_address).to_array(),
        );
        env.storage()
            .instance()
            .remove(&DataKey::Reverse(old_meta_hash));

        // Update
        let new_entry = NameEntry {
            name: name.clone(),
            stealth_meta_address: new_meta_address.clone(),
            owner,
        };
        env.storage().instance().set(&name_key, &new_entry);

        // New reverse
        let new_meta_hash =
            BytesN::from_array(&env, &env.crypto().sha256(&new_meta_address).to_array());
        env.storage()
            .instance()
            .set(&DataKey::Reverse(new_meta_hash), &name_hash);

        env.events().publish(
            (symbol_short!("register"), name_hash),
            (name, new_meta_address),
        );

        Ok(())
    }

    /// Release a name, making it available again.
    pub fn release(env: Env, owner: Address, name: String) -> Result<(), NamesError> {
        owner.require_auth();

        let name_hash = Self::hash_name(&env, &name);
        let name_key = DataKey::Name(name_hash.clone());

        let entry: NameEntry = env
            .storage()
            .instance()
            .get(&name_key)
            .ok_or(NamesError::NameNotFound)?;

        if entry.owner != owner {
            return Err(NamesError::NotOwner);
        }

        // Remove reverse
        let meta_hash = BytesN::from_array(
            &env,
            &env.crypto().sha256(&entry.stealth_meta_address).to_array(),
        );
        env.storage()
            .instance()
            .remove(&DataKey::Reverse(meta_hash));

        // Remove metadata if any
        env.storage()
            .instance()
            .remove(&DataKey::Metadata(name_hash.clone()));

        // Remove name
        env.storage().instance().remove(&name_key);

        env.events()
            .publish((symbol_short!("release"), name_hash), name);

        Ok(())
    }

    /// Resolve a name to its stealth meta-address.
    pub fn resolve(env: Env, name: String) -> Result<Bytes, NamesError> {
        let name_hash = Self::hash_name(&env, &name);
        let entry: NameEntry = env
            .storage()
            .instance()
            .get(&DataKey::Name(name_hash))
            .ok_or(NamesError::NameNotFound)?;
        Ok(entry.stealth_meta_address)
    }

    /// Reverse lookup: find the name for a given stealth meta-address.
    pub fn name_of(env: Env, stealth_meta_address: Bytes) -> Result<String, NamesError> {
        let meta_hash =
            BytesN::from_array(&env, &env.crypto().sha256(&stealth_meta_address).to_array());
        let name_hash: BytesN<32> = env
            .storage()
            .instance()
            .get(&DataKey::Reverse(meta_hash))
            .ok_or(NamesError::NameNotFound)?;
        let entry: NameEntry = env
            .storage()
            .instance()
            .get(&DataKey::Name(name_hash))
            .ok_or(NamesError::NameNotFound)?;
        Ok(entry.name)
    }

    /// Set metadata (text records + content hash) for a name.
    /// Only the current owner can set metadata.
    pub fn set_metadata(
        env: Env,
        owner: Address,
        name: String,
        metadata: MetadataEntry,
    ) -> Result<(), NamesError> {
        owner.require_auth();

        let name_hash = Self::hash_name(&env, &name);
        let name_key = DataKey::Name(name_hash.clone());

        let entry: NameEntry = env
            .storage()
            .instance()
            .get(&name_key)
            .ok_or(NamesError::NameNotFound)?;

        if entry.owner != owner {
            return Err(NamesError::NotOwner);
        }

        metadata::validate_metadata_entry(&metadata).map_err(|e| match e {
            metadata::MetadataError::MetadataKeyTooLong => NamesError::MetadataKeyTooLong,
            metadata::MetadataError::MetadataValueTooLong => NamesError::MetadataValueTooLong,
            metadata::MetadataError::MetadataRecordTooLong => NamesError::MetadataRecordTooLong,
            metadata::MetadataError::MetadataTotalTooLong => NamesError::MetadataTotalTooLong,
            metadata::MetadataError::MetadataNotFound => NamesError::MetadataNotFound,
        })?;

        env.storage()
            .instance()
            .set(&DataKey::Metadata(name_hash.clone()), &metadata);

        env.events()
            .publish((symbol_short!("mtadu"), name_hash), metadata);

        Ok(())
    }

    /// Get metadata for a name.
    /// Returns `NameNotFound` if the name does not exist,
    /// `MetadataNotFound` if the name exists but has no metadata.
    pub fn get_metadata(env: Env, name: String) -> Result<MetadataEntry, NamesError> {
        let name_hash = Self::hash_name(&env, &name);

        // Verify name exists
        let _entry: NameEntry = env
            .storage()
            .instance()
            .get(&DataKey::Name(name_hash.clone()))
            .ok_or(NamesError::NameNotFound)?;

        env.storage()
            .instance()
            .get(&DataKey::Metadata(name_hash))
            .ok_or(NamesError::MetadataNotFound)
    }

    /// Hash a name string to BytesN<32> for use as storage key.
    fn hash_name(env: &Env, name: &String) -> BytesN<32> {
        let len = name.len() as usize;
        let mut buf = [0u8; 32];
        if len > 0 {
            name.copy_into_slice(&mut buf[..len]);
        }
        let bytes = Bytes::from_slice(env, &buf[..len]);
        BytesN::from_array(env, &env.crypto().sha256(&bytes).to_array())
    }

    /// Validate name: 3-32 chars, lowercase alphanumeric only.
    fn validate_name(_env: &Env, name: &String) -> Result<(), NamesError> {
        let len = name.len() as usize;
        if len < 3 {
            return Err(NamesError::NameTooShort);
        }
        if len > 32 {
            return Err(NamesError::NameTooLong);
        }

        let mut buf = [0u8; 32];
        name.copy_into_slice(&mut buf[..len]);
        for i in 0..len {
            let c = buf[i];
            let is_lower = c >= b'a' && c <= b'z';
            let is_digit = c >= b'0' && c <= b'9';
            if !is_lower && !is_digit {
                return Err(NamesError::InvalidNameCharacter);
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::testutils::Address as _;
    use soroban_sdk::{Bytes, Env, Map, String};

    #[test]
    fn test_register_and_resolve() {
        let env = Env::default();
        env.mock_all_auths();

        let contract_id = env.register(WraithNamesContract, ());
        let client = WraithNamesContractClient::new(&env, &contract_id);

        let owner = Address::generate(&env);
        let name = String::from_str(&env, "alice");
        let meta = Bytes::from_slice(&env, &[42u8; 64]);

        client.register(&owner, &name, &meta);

        let resolved = client.resolve(&name);
        assert_eq!(resolved, meta);
    }

    #[test]
    fn test_name_taken() {
        let env = Env::default();
        env.mock_all_auths();

        let contract_id = env.register(WraithNamesContract, ());
        let client = WraithNamesContractClient::new(&env, &contract_id);

        let owner1 = Address::generate(&env);
        let owner2 = Address::generate(&env);
        let name = String::from_str(&env, "bob");
        let meta1 = Bytes::from_slice(&env, &[1u8; 64]);
        let meta2 = Bytes::from_slice(&env, &[2u8; 64]);

        client.register(&owner1, &name, &meta1);
        let result = client.try_register(&owner2, &name, &meta2);
        assert_eq!(result, Err(Ok(NamesError::NameTaken)));
    }

    #[test]
    fn test_name_of_reverse() {
        let env = Env::default();
        env.mock_all_auths();

        let contract_id = env.register(WraithNamesContract, ());
        let client = WraithNamesContractClient::new(&env, &contract_id);

        let owner = Address::generate(&env);
        let name = String::from_str(&env, "charlie");
        let meta = Bytes::from_slice(&env, &[99u8; 64]);

        client.register(&owner, &name, &meta);

        let found_name = client.name_of(&meta);
        assert_eq!(found_name, name);
    }

    #[test]
    fn test_release() {
        let env = Env::default();
        env.mock_all_auths();

        let contract_id = env.register(WraithNamesContract, ());
        let client = WraithNamesContractClient::new(&env, &contract_id);

        let owner = Address::generate(&env);
        let name = String::from_str(&env, "dave");
        let meta = Bytes::from_slice(&env, &[88u8; 64]);

        client.register(&owner, &name, &meta);
        client.release(&owner, &name);

        let result = client.try_resolve(&name);
        assert_eq!(result, Err(Ok(NamesError::NameNotFound)));

        // Can re-register after release
        let owner2 = Address::generate(&env);
        let meta2 = Bytes::from_slice(&env, &[77u8; 64]);
        client.register(&owner2, &name, &meta2);
        assert_eq!(client.resolve(&name), meta2);
    }

    #[test]
    fn test_invalid_name() {
        let env = Env::default();
        env.mock_all_auths();

        let contract_id = env.register(WraithNamesContract, ());
        let client = WraithNamesContractClient::new(&env, &contract_id);

        let owner = Address::generate(&env);
        let meta = Bytes::from_slice(&env, &[1u8; 64]);

        // Too short
        let result = client.try_register(&owner, &String::from_str(&env, "ab"), &meta);
        assert_eq!(result, Err(Ok(NamesError::NameTooShort)));

        // Invalid chars
        let result = client.try_register(&owner, &String::from_str(&env, "Alice"), &meta);
        assert_eq!(result, Err(Ok(NamesError::InvalidNameCharacter)));
    }

    // --- Metadata tests ---

    #[test]
    fn test_set_and_get_metadata() {
        let env = Env::default();
        env.mock_all_auths();

        let contract_id = env.register(WraithNamesContract, ());
        let client = WraithNamesContractClient::new(&env, &contract_id);

        let owner = Address::generate(&env);
        let name = String::from_str(&env, "eve");
        let meta = Bytes::from_slice(&env, &[1u8; 64]);
        client.register(&owner, &name, &meta);

        let mut text_records = Map::<String, String>::new(&env);
        text_records.set(
            String::from_str(&env, "avatar"),
            String::from_str(&env, "https://example.com/avatar.png"),
        );
        text_records.set(
            String::from_str(&env, "twitter"),
            String::from_str(&env, "@wraithprotocol"),
        );

        let metadata = MetadataEntry {
            text_records: text_records.clone(),
            content_hash: BytesN::from_array(&env, &[9u8; 32]),
        };

        client.set_metadata(&owner, &name, &metadata);

        let stored = client.get_metadata(&name);
        assert_eq!(stored.text_records, text_records);
        assert_eq!(stored.content_hash, BytesN::from_array(&env, &[9u8; 32]));
    }

    #[test]
    fn test_set_metadata_not_owner() {
        let env = Env::default();
        env.mock_all_auths();

        let contract_id = env.register(WraithNamesContract, ());
        let client = WraithNamesContractClient::new(&env, &contract_id);

        let owner = Address::generate(&env);
        let attacker = Address::generate(&env);
        let name = String::from_str(&env, "mallory");
        let meta = Bytes::from_slice(&env, &[2u8; 64]);
        client.register(&owner, &name, &meta);

        let text_records = Map::<String, String>::new(&env);
        let metadata = MetadataEntry {
            text_records,
            content_hash: BytesN::from_array(&env, &[0u8; 32]),
        };

        let result = client.try_set_metadata(&attacker, &name, &metadata);
        assert_eq!(result, Err(Ok(NamesError::NotOwner)));
    }

    #[test]
    fn test_set_metadata_key_too_long() {
        let env = Env::default();
        env.mock_all_auths();

        let contract_id = env.register(WraithNamesContract, ());
        let client = WraithNamesContractClient::new(&env, &contract_id);

        let owner = Address::generate(&env);
        let name = String::from_str(&env, "frank");
        let meta = Bytes::from_slice(&env, &[3u8; 64]);
        client.register(&owner, &name, &meta);

        // Key exactly 65 bytes (MAX 64) → should fail
        let long_key = String::from_str(
            &env,
            "012345678901234567890123456789012345678901234567890123456789012345",
        );
        let mut text_records = Map::<String, String>::new(&env);
        text_records.set(long_key, String::from_str(&env, "x"));

        let metadata = MetadataEntry {
            text_records,
            content_hash: BytesN::from_array(&env, &[0u8; 32]),
        };

        let result = client.try_set_metadata(&owner, &name, &metadata);
        assert_eq!(result, Err(Ok(NamesError::MetadataKeyTooLong)));
    }

    #[test]
    fn test_set_metadata_value_too_long() {
        let env = Env::default();
        env.mock_all_auths();

        let contract_id = env.register(WraithNamesContract, ());
        let client = WraithNamesContractClient::new(&env, &contract_id);

        let owner = Address::generate(&env);
        let name = String::from_str(&env, "grace");
        let meta = Bytes::from_slice(&env, &[4u8; 64]);
        client.register(&owner, &name, &meta);

        // Value exactly 257 bytes (MAX 256) → should fail
        let long_value = String::from_str(&env, &"x".repeat(257));
        let mut text_records = Map::<String, String>::new(&env);
        text_records.set(String::from_str(&env, "avatar"), long_value);

        let metadata = MetadataEntry {
            text_records,
            content_hash: BytesN::from_array(&env, &[0u8; 32]),
        };

        let result = client.try_set_metadata(&owner, &name, &metadata);
        assert_eq!(result, Err(Ok(NamesError::MetadataValueTooLong)));
    }

    #[test]
    fn test_set_metadata_total_too_long() {
        let env = Env::default();
        env.mock_all_auths();

        let contract_id = env.register(WraithNamesContract, ());
        let client = WraithNamesContractClient::new(&env, &contract_id);

        let owner = Address::generate(&env);
        let name = String::from_str(&env, "heidi");
        let meta = Bytes::from_slice(&env, &[5u8; 64]);
        client.register(&owner, &name, &meta);

        let mut text_records = Map::<String, String>::new(&env);
        // 5 records each ~210 bytes (key=5, value=205) = 1050 total > 1024 limit
        // Each record stays under the 256-byte per-value limit
        text_records.set(
            String::from_str(&env, "key00"),
            String::from_str(&env, &"v".repeat(205)),
        );
        text_records.set(
            String::from_str(&env, "key01"),
            String::from_str(&env, &"v".repeat(205)),
        );
        text_records.set(
            String::from_str(&env, "key02"),
            String::from_str(&env, &"v".repeat(205)),
        );
        text_records.set(
            String::from_str(&env, "key03"),
            String::from_str(&env, &"v".repeat(205)),
        );
        text_records.set(
            String::from_str(&env, "key04"),
            String::from_str(&env, &"v".repeat(205)),
        );

        let metadata = MetadataEntry {
            text_records,
            content_hash: BytesN::from_array(&env, &[0u8; 32]),
        };

        let result = client.try_set_metadata(&owner, &name, &metadata);
        assert_eq!(result, Err(Ok(NamesError::MetadataTotalTooLong)));
    }

    #[test]
    fn test_get_metadata_not_found() {
        let env = Env::default();
        env.mock_all_auths();

        let contract_id = env.register(WraithNamesContract, ());
        let client = WraithNamesContractClient::new(&env, &contract_id);

        let owner = Address::generate(&env);
        let name = String::from_str(&env, "ivan");
        let meta = Bytes::from_slice(&env, &[6u8; 64]);
        client.register(&owner, &name, &meta);

        // Name exists but no metadata set
        let result = client.try_get_metadata(&name);
        assert_eq!(result, Err(Ok(NamesError::MetadataNotFound)));
    }

    #[test]
    fn test_get_metadata_unregistered_name() {
        let env = Env::default();

        let contract_id = env.register(WraithNamesContract, ());
        let client = WraithNamesContractClient::new(&env, &contract_id);

        let name = String::from_str(&env, "nobody");
        let result = client.try_get_metadata(&name);
        assert_eq!(result, Err(Ok(NamesError::NameNotFound)));
    }

    #[test]
    fn test_metadata_cleaned_on_release() {
        let env = Env::default();
        env.mock_all_auths();

        let contract_id = env.register(WraithNamesContract, ());
        let client = WraithNamesContractClient::new(&env, &contract_id);

        let owner = Address::generate(&env);
        let name = String::from_str(&env, "judy");
        let meta = Bytes::from_slice(&env, &[7u8; 64]);
        client.register(&owner, &name, &meta);

        let mut text_records = Map::<String, String>::new(&env);
        text_records.set(
            String::from_str(&env, "avatar"),
            String::from_str(&env, "https://example.com/pic.jpg"),
        );
        let metadata = MetadataEntry {
            text_records,
            content_hash: BytesN::from_array(&env, &[0u8; 32]),
        };

        client.set_metadata(&owner, &name, &metadata);

        // Verify metadata exists
        let stored = client.get_metadata(&name);
        assert_eq!(stored.text_records.len(), 1);

        // Release should clean metadata
        client.release(&owner, &name);

        // After release, get_metadata should return NameNotFound (name is gone)
        let result = client.try_get_metadata(&name);
        assert_eq!(result, Err(Ok(NamesError::NameNotFound)));
    }

    #[test]
    fn test_hot_path_unchanged_after_metadata() {
        let env = Env::default();
        env.mock_all_auths();

        let contract_id = env.register(WraithNamesContract, ());
        let client = WraithNamesContractClient::new(&env, &contract_id);

        let owner = Address::generate(&env);
        let name = String::from_str(&env, "karen");
        let meta = Bytes::from_slice(&env, &[8u8; 64]);
        client.register(&owner, &name, &meta);

        // Set metadata
        let text_records = Map::<String, String>::new(&env);
        let metadata = MetadataEntry {
            text_records,
            content_hash: BytesN::from_array(&env, &[0u8; 32]),
        };
        client.set_metadata(&owner, &name, &metadata);

        // Hot path: resolve still works
        let resolved = client.resolve(&name);
        assert_eq!(resolved, meta);

        // Hot path: name_of still works
        let found_name = client.name_of(&meta);
        assert_eq!(found_name, name);
    }
}
