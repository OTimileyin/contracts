# wraith-names metadata resolver

This package extends the existing `.wraith` name registry with optional on-chain metadata. The resolution path remains unchanged for the existing `resolve(name)` call, while metadata is exposed through dedicated owner-gated setters/getters.

## Supported metadata

- `text_records`: key/value text mappings such as `avatar`, `twitter`, and `description`
- `content_hash`: optional `BytesN<32>` content identifier for IPFS or similar content-addressed payloads

## Design notes

- Metadata is opt-in and does not change the existing name-to-meta-address `resolve` hot path.
- Ownership is enforced by the name owner before metadata storage can be updated.
- Event emission should be used to notify listeners when metadata is changed.

## Validation

- Per-record size limits are enforced.
- Total metadata payload size must remain within the configured limit.
- Invalid metadata size must return dedicated typed errors instead of falling through to generic resolution errors.
