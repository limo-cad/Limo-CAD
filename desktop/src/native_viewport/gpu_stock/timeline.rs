//! Require agreement from all timed layers of the active playback path.

pub(super) fn single_tool(flags: impl IntoIterator<Item = bool>) -> bool {
    let mut flags = flags.into_iter();
    flags.next() == Some(true) && flags.all(|single_tool| single_tool)
}

#[cfg(test)]
mod tests {
    use super::single_tool;

    #[test]
    fn no_matching_timeline_cannot_enable_removal() {
        assert!(!single_tool([]));
    }

    #[test]
    fn all_matching_layers_must_prove_one_tool() {
        assert!(single_tool([true]));
        assert!(single_tool([true, true]));
        assert!(!single_tool([true, false]));
        assert!(!single_tool([false, true]));
        assert!(!single_tool([false, false]));
    }
}
