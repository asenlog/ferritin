//! Static port adapters for integration tests — Null Objects that
//! satisfy a port with a fixed list. No logic, ever: a static
//! adapter that grows behavior belongs in a real module.
//!
//! Newtypes rather than impls on `Vec`: the orphan rule forbids
//! implementing a foreign trait for a foreign type from a test crate.

use ferritin_core::domain::auth::AuthorizedCaller;
use ferritin_core::domain::rules::ForwardingRule;
use ferritin_core::ports::{CallerDirectory, RuleDirectory};

// each test crate compiles this module and uses only what it needs
#[allow(dead_code)]
pub struct StaticCallers(pub Vec<AuthorizedCaller>);

impl CallerDirectory for StaticCallers {
    fn authorized_callers(&self) -> anyhow::Result<Vec<AuthorizedCaller>> {
        Ok(self.0.clone())
    }
}

#[allow(dead_code)]
pub struct StaticRules(pub Vec<ForwardingRule>);

impl RuleDirectory for StaticRules {
    fn forwarding_rules(&self) -> anyhow::Result<Vec<ForwardingRule>> {
        Ok(self.0.clone())
    }
}
