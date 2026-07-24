use soroban_sdk::{contracterror, contracttype, BytesN, Env, Map, String};

pub const MAX_TEXT_RECORD_KEY_BYTES: u32 = 64;
pub const MAX_TEXT_RECORD_VALUE_BYTES: u32 = 256;
pub const MAX_METADATA_TOTAL_BYTES: u32 = 1024;

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MetadataEntry {
    /// Optional text records keyed by names such as "avatar", "twitter", or "description".
    pub text_records: Map<String, String>,
    /// Optional content hash for IPFS or similar content-addressed payloads.
    pub content_hash: BytesN<32>,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum MetadataError {
    MetadataKeyTooLong = 101,
    MetadataValueTooLong = 102,
    MetadataRecordTooLong = 103,
    MetadataTotalTooLong = 104,
    MetadataNotFound = 105,
}

pub const METADATA_UPDATED_EVENT: &str = "MetadataUpdated";

pub fn validate_text_record(key: &String, value: &String) -> Result<(), MetadataError> {
    if key.len() > MAX_TEXT_RECORD_KEY_BYTES {
        return Err(MetadataError::MetadataKeyTooLong);
    }

    if value.len() > MAX_TEXT_RECORD_VALUE_BYTES {
        return Err(MetadataError::MetadataValueTooLong);
    }

    let record_len = key.len().checked_add(value.len()).ok_or(MetadataError::MetadataRecordTooLong)?;
    if record_len > MAX_METADATA_TOTAL_BYTES {
        return Err(MetadataError::MetadataRecordTooLong);
    }

    Ok(())
}

pub fn validate_text_records(text_records: &Map<String, String>) -> Result<(), MetadataError> {
    let mut total = 0u32;

    for (key, value) in text_records.iter() {
        validate_text_record(&key, &value)?;

        total = total
            .checked_add(key.len())
            .ok_or(MetadataError::MetadataTotalTooLong)?;
        total = total
            .checked_add(value.len())
            .ok_or(MetadataError::MetadataTotalTooLong)?;
    }

    if total > MAX_METADATA_TOTAL_BYTES {
        return Err(MetadataError::MetadataTotalTooLong);
    }

    Ok(())
}

pub fn validate_metadata_entry(entry: &MetadataEntry) -> Result<(), MetadataError> {
    validate_text_records(&entry.text_records)?;

    if entry.content_hash.len() != 32 {
        return Err(MetadataError::MetadataRecordTooLong);
    }

    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::{BytesN, Env, String};

    #[test]
    fn test_metadata_entry_validation_accepts_opt_in_fields() {
        let env = Env::default();

        let mut text_records = Map::<String, String>::new(&env);
        text_records.set(
            &String::from_str(&env, "avatar"),
            &String::from_str(&env, "https://example.com/avatar.png"),
        );
        text_records.set(
            &String::from_str(&env, "twitter"),
            &String::from_str(&env, "@wraithprotocol"),
        );

        let metadata = MetadataEntry {
            text_records,
            content_hash: BytesN::from_array(&env, &[9u8; 32]),
        };

        assert_eq!(validate_metadata_entry(&metadata), Ok(()));
    }

    #[test]
    fn test_metadata_key_limit_is_enforced() {
        let env = Env::default();
        let key = String::from_str(&env, "a");
        let value = String::from_str(&env, "x");

        let long_key = String::from_str(&env, "012345678901234567890123456789012345678901234567890123456789012345");
        assert_eq!(validate_text_record(&key, &value), Ok(()));
        assert_eq!(validate_text_record(&long_key, &value), Err(MetadataError::MetadataKeyTooLong));
    }

    #[test]
    fn test_metadata_total_limit_is_enforced() {
        let env = Env::default();
        let mut text_records = Map::<String, String>::new(&env);

        text_records.set(
            &String::from_str(&env, "description"),
            &String::from_str(&env, "012345678901234567890123456789012345678901234567890123456789012345"),
        );
        text_records.set(
            &String::from_str(&env, "avatar"),
            &String::from_str(&env, "012345678901234567890123456789012345678901234567890123456789012345"),
        );

        assert_eq!(validate_text_records(&text_records), Err(MetadataError::MetadataTotalTooLong));
    }
}
