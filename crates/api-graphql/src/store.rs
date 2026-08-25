/// Resolves the effective store ID from the three places this GraphQL layer
/// accepts it, in priority order: the `Store` request header, the
/// `__Store` GraphQL variable in the request body, then the `__Store` query
/// parameter. The first source that parses to a valid `u16` wins; falls
/// back to `0` (the default/admin store) if none do.
pub fn resolve_store_id(header: Option<&str>, body_variable: Option<&str>, query_param: Option<&str>) -> u16 {
    header
        .and_then(|s| s.parse().ok())
        .or_else(|| body_variable.and_then(|s| s.parse().ok()))
        .or_else(|| query_param.and_then(|s| s.parse().ok()))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_takes_priority_over_everything() {
        assert_eq!(resolve_store_id(Some("2"), Some("3"), Some("4")), 2);
    }

    #[test]
    fn body_variable_wins_when_header_is_absent() {
        assert_eq!(resolve_store_id(None, Some("3"), Some("4")), 3);
    }

    #[test]
    fn query_param_is_the_last_resort() {
        assert_eq!(resolve_store_id(None, None, Some("4")), 4);
    }

    #[test]
    fn defaults_to_zero_when_nothing_is_provided() {
        assert_eq!(resolve_store_id(None, None, None), 0);
    }

    #[test]
    fn falls_through_on_unparseable_values() {
        assert_eq!(resolve_store_id(Some("not-a-number"), Some("3"), None), 3);
        assert_eq!(resolve_store_id(Some("not-a-number"), Some("also-not-a-number"), Some("4")), 4);
    }
}
