use sandbox_common::{Result, SandboxPolicy};

pub fn parse_policy_json(input: &str) -> Result<SandboxPolicy> {
    SandboxPolicy::from_json(input)
}
