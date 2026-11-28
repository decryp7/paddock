//! The repository files that tools of paddock-dev read and write, named once (rule 40, ER-22): the register of the CI dispatch
//! guards, the fix-item home and the test-cache configuration. They live in the repo layer, beside [`crate::escape::register::FILE`]
//! and [`crate::run_scope::REGISTER`], so the check that lists the registers tools write (C177) and the ci modules that write them
//! read the same constants and neither points up at the other.

/// Where each dispatch guard's first green use is recorded (ER-41), relative to the repository root.
pub const DISPATCH_GUARDS: &str = "docs/ci-dispatch-guards.toml";

/// The fix-item home: the lessons file of the repository.
pub const LESSONS: &str = "docs/lessons.md";

/// The test-cache configuration, repository-relative.
pub const TEST_CACHE: &str = "docs/test-cache.toml";
