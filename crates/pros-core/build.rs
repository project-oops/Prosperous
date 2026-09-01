//! Stamps the commit this binary was built from.
//!
//! The work is `oops_build::emit` - shared, because this was written here and in orbistoun and
//! the two had drifted into fixing different halves of it. What was wrong *here* is worth
//! keeping written down: the commit came from `PROSPEROUS_COMMIT`, and nothing ever set it -
//! not CI, not `bin/prosperous`, nothing in the repository outside the script that read it. Every
//! binary this project has produced was stamped `no commit`, and nothing noticed, because that
//! is exactly what a local build looks like.
//!
//! The shared version asks git when nothing tells it, so there is no configuration to forget.
fn main() {
    oops_build::emit();
}
