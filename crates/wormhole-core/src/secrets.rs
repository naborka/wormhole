//! The host-side secret store: values asked for once — a key ten roles
//! share, typed one time — and handed to every box whose manifest asks
//! for them by name. One flat file of `NAME = "value"` lines, owned by
//! the person and never by a role, which is why nothing here knows what
//! a manifest is. The binary reads and writes the file; this is the one
//! body for what its bytes mean.

use std::collections::BTreeMap;

/// What the store file holds. Same shape as a host environment, so
/// `boxenv` treats both under one rule: an empty value is no value.
pub type Store = BTreeMap<String, String>;

pub fn parse(text: &str) -> Result<Store, String> {
    toml::from_str(text).map_err(|e| format!("the secret store is not valid: {}", e.message()))
}

pub fn to_toml(store: &Store) -> Result<String, String> {
    toml::to_string(store).map_err(|e| format!("cannot serialize the secret store: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_store_survives_the_toml_round_trip() {
        let store: Store = [("CONTEXT7_API_KEY", "k"), ("SENTRY_TOKEN", "t")]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .collect();
        let text = to_toml(&store).expect("serializes");
        assert_eq!(parse(&text).expect("parses"), store);
        assert_eq!(parse("").expect("an empty file is an empty store"), Store::new());
    }

    #[test]
    fn a_broken_store_names_itself_not_a_serde_detail() {
        let error = parse("NAME = 5").expect_err("a number is not a secret");
        assert!(error.contains("secret store"), "{error}");
    }
}
