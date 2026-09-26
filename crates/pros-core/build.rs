//! Stamps the commit this binary was built from, through `oops_build::emit`, which asks git
//! when nothing in the environment names the commit.
fn main() {
    oops_build::emit();
}
