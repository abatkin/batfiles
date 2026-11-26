//! `facts list`: print the host facts. See
//! [`docs/commands/facts.md`](../../docs/commands/facts.md).

use crate::manifest::condition;
use crate::output::{Reporter, quoted_value};

/// Print each host fact on standard output as `key = "value"`, keys sorted and aligned. Reads
/// no repository and no state.
pub(crate) fn list(reporter: &Reporter) {
    let facts = condition::facts();
    let width = facts.keys().map(String::len).max().unwrap_or_default();
    for (key, value) in &facts {
        reporter.data(&format!("{key:<width$} = {}", quoted_value(value)));
    }
}
