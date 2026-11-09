//! The build-id check for the pinned releases' asset names (the FFmpeg runtime and the research browser's download use
//! it); the llama.cpp build ids themselves are gone with the built-in engine (ADR 0070).

/// Build ids are folder names: short, lowercase ASCII letters, digits, `.` and `-`.
pub fn is_build_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 32
        && !id.starts_with('.')
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_ids_are_short_safe_folder_names() {
        assert!(
            is_build_id("cuda-12.4") && is_build_id("b7") && !is_build_id(".hidden") && !is_build_id(&"a".repeat(33)) && !is_build_id("")
        );
        assert!(!is_build_id("../x") && !is_build_id("Vulkan"));
    }
}
