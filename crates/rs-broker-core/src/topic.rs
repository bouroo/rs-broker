//! MQTT-standard topic pattern matching.
//!
//! Topics and patterns are split into segments on `.`. The matcher walks the
//! topic and pattern segments in lockstep, recursing on the tail of each.
//!
//! Wildcard semantics:
//! - `+` as a full segment → matches exactly one segment.
//! - `#` as a full segment → matches zero or more remaining segments
//!   (only valid as the last segment; if it appears, everything after
//!   matches).
//! - `*` as a full segment → alias for `+` (matches exactly one segment).
//!   This preserves backward-compat with existing subscriber patterns like
//!   `orders.*`.
//! - `*` appearing INSIDE a segment (e.g. `order*created`, `ship*`) → simple
//!   glob: the segment must start with the literal prefix that precedes the
//!   first `*`.
//! - Literal segment → must equal the topic segment exactly.
//! - Empty topic and empty pattern: `""` matches `""`. `""` does not match
//!   `"user"`. `"user"` does not match `""`.

/// Returns `true` if `topic` matches the single `pattern`.
///
/// See the [module-level documentation](self) for the wildcard contract.
pub fn matches_topic(topic: &str, pattern: &str) -> bool {
    if pattern.is_empty() {
        return topic.is_empty();
    }
    if topic.is_empty() {
        return pattern == "#";
    }
    match_segments(topic.as_bytes(), pattern.as_bytes())
}

/// Returns `true` if `topic` matches any pattern in `patterns`.
pub fn matches_any(topic: &str, patterns: &[String]) -> bool {
    patterns.iter().any(|p| matches_topic(topic, p))
}

fn match_segments(topic: &[u8], pattern: &[u8]) -> bool {
    let p_seg = match next_segment(pattern) {
        Some(s) => s,
        None => return next_segment(topic).is_none(),
    };

    if p_seg.bytes == b"#" {
        return true;
    }

    let t_seg = match next_segment(topic) {
        Some(s) => s,
        None => return false,
    };

    let t_rest = &topic[t_seg.end..];
    let p_rest = &pattern[p_seg.end..];

    if p_seg.bytes == b"+" || p_seg.bytes == b"*" {
        return match_segments(t_rest, p_rest);
    }
    if p_seg.bytes == t_seg.bytes {
        return match_segments(t_rest, p_rest);
    }
    if p_seg.bytes.contains(&b'*') {
        // Inline `*` inside a pattern segment is treated as a prefix-glob
        // against the topic string: the topic must start with the literal
        // bytes that precede the first `*`.
        let prefix_end = p_seg
            .bytes
            .iter()
            .position(|&b| b == b'*')
            .unwrap_or(p_seg.bytes.len());
        let prefix = &p_seg.bytes[..prefix_end];
        return topic.starts_with(prefix);
    }
    false
}

struct Segment<'a> {
    bytes: &'a [u8],
    end: usize,
}

fn next_segment(input: &[u8]) -> Option<Segment<'_>> {
    match input.iter().position(|&b| b == b'.') {
        Some(idx) => Some(Segment {
            bytes: &input[..idx],
            end: idx + 1,
        }),
        None if input.is_empty() => None,
        None => Some(Segment {
            bytes: input,
            end: input.len(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Exact match ----
    #[test]
    fn exact_match() {
        assert!(matches_topic("user.created", "user.created"));
        assert!(matches_topic("order.updated", "order.updated"));
        assert!(!matches_topic("user.created", "user.updated"));
    }

    // ---- '*' as full segment (alias for '+') ----
    #[test]
    fn star_full_segment_matches_one() {
        assert!(matches_topic("user.created", "user.*"));
        assert!(matches_topic("user.updated", "user.*"));
        assert!(matches_topic("user.deleted", "user.*"));
    }

    #[test]
    fn star_full_segment_does_not_match_multi() {
        assert!(!matches_topic("user.profile.updated", "user.*"));
    }

    #[test]
    fn star_full_segment_does_not_match_other_prefix() {
        assert!(!matches_topic("order.created", "user.*"));
    }

    // ---- '+' = one segment ----
    #[test]
    fn plus_matches_one_segment() {
        assert!(matches_topic("user.created", "user.+"));
        assert!(matches_topic("user.updated", "user.+"));
        assert!(matches_topic("order.shipped", "order.+"));
    }

    #[test]
    fn plus_does_not_match_multi() {
        assert!(!matches_topic("user.profile.updated", "user.+"));
    }

    // ---- '#' = multi-segment suffix ----
    #[test]
    fn hash_matches_multi_segment_suffix() {
        assert!(matches_topic("user.profile.updated", "user.#"));
        assert!(matches_topic("user.created", "user.#"));
    }

    #[test]
    fn hash_matches_zero_segments() {
        assert!(matches_topic("user", "user.#"));
    }

    #[test]
    fn hash_at_root_matches_everything() {
        assert!(matches_topic("a.b.c.d", "#"));
        assert!(matches_topic("a", "#"));
        assert!(matches_topic("", "#"));
    }

    // ---- '*' / '+' mixed in middle ----
    #[test]
    fn star_middle_matches_one_segment() {
        assert!(matches_topic("user.profile.updated", "user.*.updated"));
        assert!(matches_topic("a.b.c.d", "a.*.c.d"));
    }

    #[test]
    fn plus_middle_matches_one_segment() {
        assert!(matches_topic("a.b.c.d", "a.+.c.+"));
        assert!(matches_topic("event.user.created", "event.+.+"));
    }

    // ---- Inline '*' within a segment (glob) ----
    #[test]
    fn inline_star_prefix_glob() {
        assert!(matches_topic("orders.created", "order*"));
        assert!(matches_topic("order", "order*"));
    }

    #[test]
    fn inline_star_prefix_glob_no_match() {
        assert!(!matches_topic("payments.created", "order*"));
    }

    // ---- Empty strings ----
    #[test]
    fn empty_topic_and_pattern_match() {
        assert!(matches_topic("", ""));
    }

    #[test]
    fn empty_topic_does_not_match_nonempty_pattern() {
        assert!(!matches_topic("", "user"));
        assert!(!matches_topic("", "user.*"));
    }

    #[test]
    fn nonempty_topic_does_not_match_empty_pattern() {
        assert!(!matches_topic("user", ""));
    }

    // ---- Realistic ----
    #[test]
    fn realistic_service_topic() {
        assert!(matches_topic("user.service.created", "user.service.*"));
    }

    #[test]
    fn realistic_payment_topic() {
        assert!(matches_topic(
            "order.payment.completed",
            "order.*.completed"
        ));
    }

    // ---- Anchor: previously passing inbox cases ----
    #[test]
    fn anchor_no_match_different_segments() {
        assert!(!matches_topic("user.created.now", "user.created"));
        assert!(!matches_topic("user", "user.created"));
        assert!(!matches_topic("created.user", "user.created"));
    }

    #[test]
    fn anchor_complex_patterns() {
        assert!(matches_topic("user.profile.updated", "user.*.updated"));
        assert!(matches_topic("order.items.created", "order.*.created"));
        assert!(matches_topic("event.user.created", "event.+.+"));
    }

    #[test]
    fn anchor_realistic_scenarios() {
        assert!(matches_topic(
            "user.service.created",
            "user.service.created"
        ));
        assert!(matches_topic("user.service.created", "user.service.*"));
        assert!(matches_topic("user.service.created", "user.*.created"));
        assert!(matches_topic(
            "order.payment.completed",
            "order.payment.completed"
        ));
        assert!(matches_topic(
            "order.payment.completed",
            "order.*.completed"
        ));
        assert!(matches_topic(
            "inventory.stock.updated",
            "inventory.stock.*"
        ));
    }

    // ---- matches_any ----
    #[test]
    fn matches_any_returns_true_on_first_hit() {
        let patterns = vec![
            "payments.*".to_string(),
            "orders.created".to_string(),
            "orders.*".to_string(),
        ];
        assert!(matches_any("orders.created", &patterns));
    }

    #[test]
    fn matches_any_returns_false_when_no_hit() {
        let patterns = vec!["payments.*".to_string(), "users.*".to_string()];
        assert!(!matches_any("orders.created", &patterns));
    }

    #[test]
    fn matches_any_empty_patterns_is_false() {
        let patterns: Vec<String> = vec![];
        assert!(!matches_any("orders.created", &patterns));
    }
}
