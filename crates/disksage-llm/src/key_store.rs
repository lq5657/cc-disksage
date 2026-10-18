use crate::error::LlmError;

const SERVICE: &str = "cc-disksage.llm";

fn entry(provider_id: &str) -> Result<keyring::Entry, LlmError> {
    keyring::Entry::new(SERVICE, provider_id).map_err(|error| LlmError::Keyring(error.to_string()))
}

pub fn set_key(provider_id: &str, key: &str) -> Result<(), LlmError> {
    let key = key.trim();
    if key.is_empty() {
        return Err(LlmError::Config("API key must not be empty".into()));
    }
    entry(provider_id)?
        .set_password(key)
        .map_err(|error| LlmError::Keyring(error.to_string()))
}

pub fn get_key(provider_id: &str) -> Result<Option<String>, LlmError> {
    match entry(provider_id)?.get_password() {
        Ok(key) => Ok(Some(key)),
        Err(error) => {
            let message = error.to_string().to_ascii_lowercase();
            if message.contains("no entry")
                || message.contains("not found")
                || message.contains("no credential")
                || message.contains("matching credential")
            {
                Ok(None)
            } else {
                Err(LlmError::Keyring(message))
            }
        }
    }
}

pub fn delete_key(provider_id: &str) -> Result<(), LlmError> {
    match entry(provider_id)?.delete_credential() {
        Ok(()) => Ok(()),
        Err(error) => {
            let message = error.to_string().to_ascii_lowercase();
            if message.contains("no entry")
                || message.contains("not found")
                || message.contains("no credential")
                || message.contains("matching credential")
            {
                Ok(())
            } else {
                Err(LlmError::Keyring(message))
            }
        }
    }
}
