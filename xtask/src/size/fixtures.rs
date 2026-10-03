//! Generated sources for the size tests.

/// `n` lines of non-test code.
pub fn code(n: usize) -> String {
    "pub const C: u32 = 0;\n".repeat(n)
}

/// An inline `#[cfg(test)]` module spanning exactly `n` lines.
pub fn inline_tests(n: usize) -> String {
    assert!(n >= 3);
    format!(
        "#[cfg(test)]\nmod tests {{\n{}}}\n",
        "    const T: u32 = 0;\n".repeat(n - 3)
    )
}
