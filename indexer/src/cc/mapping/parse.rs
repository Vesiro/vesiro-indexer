use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum ParseUrnUuidError {
    #[error("invalid URN UUID format: {0}")]
    InvalidFormat(String),
}

/// Parses a URN UUID string (e.g., "<urn:uuid:123e4567-e89b-12d3-a456-426614174000>") into a Uuid.
pub fn parse_urn_uuid(urn_uuid: &str) -> anyhow::Result<Uuid> {
    if !urn_uuid.starts_with("<urn:uuid:") || !urn_uuid.ends_with('>') || urn_uuid.len() != 47 {
        return Err(ParseUrnUuidError::InvalidFormat(urn_uuid.to_string()).into());
    }
    let uuid = &urn_uuid[10..46];
    let uuid = Uuid::parse_str(uuid)?;
    Ok(uuid)
}
