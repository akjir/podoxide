// Podoxide
// Copyright (C) 2026  Stefan Stark
//
// This program is free software: you can redistribute it and/or modify it under
// the terms of the GNU General Public License as published by the Free Software
// Foundation, either version 3 of the License, or (at your option)
// any later version.
//
// This program is distributed in the hope that it will be useful, but WITHOUT
// ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS
// FOR A PARTICULAR PURPOSE.  See the GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License along with
// this program.  If not, see <https://www.gnu.org/licenses/>.

//! Slice and vector helpers for option token splitting, order-preserving deduplication, and bounded pre-allocation.

use std::collections::BTreeSet;

/// Maximum initial capacity pre-allocated for dynamically sized collections.
pub const MAX_COLLECTION_PREALLOC: usize = 1024;

/// Appends `option` directive tokens from `options` into `arguments`, splitting combined `--flag value` entries.
pub fn append_option_tokens(arguments: &mut Vec<String>, options: &[String]) {
    for option in options {
        append_single_option(arguments, option);
    }
}

/// Returns `requested` clamped to `maximum` for bounded collection pre-allocation.
#[inline]
#[must_use]
pub const fn bounded_capacity(requested: usize, maximum: usize) -> usize {
    if requested < maximum {
        requested
    } else {
        maximum
    }
}

/// Returns a new vector containing the unique strings from `items` in first-appearance order.
#[must_use]
pub fn dedup_preserve_order(items: &[String]) -> Vec<String> {
    if items.is_empty() {
        return Vec::new();
    }
    let capacity = bounded_capacity(items.len(), MAX_COLLECTION_PREALLOC);
    let mut result = Vec::with_capacity(capacity);
    let mut seen = BTreeSet::new();
    for item in items {
        if seen.insert(item.as_str()) {
            result.push(item.clone());
        }
    }
    result
}

/// Appends a single `option` directive token (or whitespace-split `--flag value` pair) to `arguments`.
fn append_single_option(arguments: &mut Vec<String>, option: &str) {
    let trimmed = option.trim();
    if trimmed.starts_with('-') && trimmed.contains(char::is_whitespace) {
        arguments.extend(trimmed.split_whitespace().map(str::to_owned));
    } else {
        arguments.push(option.to_owned());
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_COLLECTION_PREALLOC, append_option_tokens, bounded_capacity, dedup_preserve_order,
    };

    #[test]
    fn append_option_tokens_splits_combined_flags_and_preserves_positional_values() {
        let mut arguments = Vec::new();
        let options = vec![
            String::from("--unknown thing"),
            String::from("--another"),
            String::from("unknown value"),
        ];
        append_option_tokens(&mut arguments, &options);
        assert_eq!(
            arguments,
            vec!["--unknown", "thing", "--another", "unknown value"]
        );
    }

    #[test]
    fn bounded_capacity_clamps_to_maximum() {
        assert_eq!(bounded_capacity(0, MAX_COLLECTION_PREALLOC), 0);
        assert_eq!(bounded_capacity(16, MAX_COLLECTION_PREALLOC), 16);
        assert_eq!(
            bounded_capacity(MAX_COLLECTION_PREALLOC, MAX_COLLECTION_PREALLOC),
            MAX_COLLECTION_PREALLOC
        );
        assert_eq!(
            bounded_capacity(MAX_COLLECTION_PREALLOC + 500, MAX_COLLECTION_PREALLOC),
            MAX_COLLECTION_PREALLOC
        );
    }

    #[test]
    fn dedup_preserve_order_retains_first_appearance() {
        let empty: Vec<String> = Vec::new();
        assert!(dedup_preserve_order(&empty).is_empty());

        let items = vec![
            String::from("lie"),
            String::from("cake"),
            String::from("the"),
            String::from("cake"),
            String::from("lie"),
        ];
        assert_eq!(
            dedup_preserve_order(&items),
            vec![
                String::from("lie"),
                String::from("cake"),
                String::from("the"),
            ]
        );
    }
}
