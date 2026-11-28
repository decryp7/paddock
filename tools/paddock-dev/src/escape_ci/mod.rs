//! The CI edges of the escape monitor ([`crate::escape`]): the plan record's artifact (written by the plan job, read from the Actions API
//! by the nightly: [`artifact`]) and the nightly's `dev plan --escapes` and the plan job's record ([`cmd`]). They read CI runs and call
//! the planner's command, so they live in the ci layer; the record, the judgement and the register stay in the repo layer.

pub mod artifact;
pub mod cmd;
#[cfg(test)]
mod tests;
