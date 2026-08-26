//! Pure parsing of the host files doctor probes read. The impl crate reads
//! the file; every decision about its content is made and tested here.

/// `/proc/sys/user/max_user_namespaces` content allows creating one.
pub fn max_user_namespaces_ok(content: &str) -> bool {
    content.trim().parse::<u64>().is_ok_and(|n| n > 0)
}

/// `/etc/subuid` or `/etc/subgid` has a range for this user, matched by
/// name or numeric uid (both forms are valid in the file).
pub fn has_subid_range(content: &str, user: &str, uid: u32) -> bool {
    let uid = uid.to_string();
    content.lines().any(|line| {
        let mut fields = line.split(':');
        let owner = fields.next().unwrap_or("");
        let count_ok = fields
            .nth(1)
            .and_then(|c| c.trim().parse::<u64>().ok())
            .is_some_and(|c| c > 0);
        (owner == user || owner == uid) && count_ok
    })
}

/// A delegated `cgroup.controllers` file grants everything wormhole limits.
pub fn missing_cgroup_controllers(content: &str) -> Vec<&'static str> {
    const NEEDED: [&str; 3] = ["cpu", "memory", "pids"];
    let present: Vec<&str> = content.split_whitespace().collect();
    NEEDED
        .into_iter()
        .filter(|c| !present.contains(c))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn max_user_namespaces_positive_ok_zero_and_garbage_not() {
        assert!(max_user_namespaces_ok("63414\n"));
        assert!(!max_user_namespaces_ok("0\n"));
        assert!(!max_user_namespaces_ok(""));
        assert!(!max_user_namespaces_ok("not-a-number\n"));
    }

    #[test]
    fn subid_range_matches_by_name_or_uid() {
        let content = "root:100000:65536\nnabor:165536:65536\n";
        assert!(has_subid_range(content, "nabor", 1000));
        assert!(!has_subid_range(content, "alice", 1001));
        assert!(has_subid_range("1000:165536:65536\n", "nabor", 1000));
    }

    #[test]
    fn subid_range_with_zero_count_does_not_count() {
        assert!(!has_subid_range("nabor:165536:0\n", "nabor", 1000));
    }

    #[test]
    fn subid_range_empty_or_malformed_is_absent() {
        assert!(!has_subid_range("", "nabor", 1000));
        assert!(!has_subid_range("nabor\n", "nabor", 1000));
        assert!(!has_subid_range("nabor:165536\n", "nabor", 1000));
    }

    #[test]
    fn cgroup_controllers_reports_what_is_missing() {
        assert!(missing_cgroup_controllers("cpuset cpu io memory pids\n").is_empty());
        assert_eq!(
            missing_cgroup_controllers("cpuset io memory\n"),
            vec!["cpu", "pids"]
        );
        assert_eq!(
            missing_cgroup_controllers(""),
            vec!["cpu", "memory", "pids"]
        );
    }
}
