//! Where a role comes from when it is not a directory on this machine: a
//! git URL and the commit that is the pin. Pure decisions only — the
//! fetching, and every filesystem question, belong to the binary.

use std::fmt;

/// How many characters a git commit is written in. Full length only: a
/// short one is ambiguous locally and refused by most servers on fetch,
/// and a pin that might name two commits is not a pin.
const SHA_LEN: usize = 40;

/// The prefixes that mean "this is a repository", checked before the path
/// rule. Every one of them carries a `/`, so without this a URL would be
/// read as a directory.
///
/// `file://` is here because git treats it as a transport like any other
/// and the pin is verified the same way — a repository on a mounted share
/// or a local mirror is not a lesser kind of source. `http://` is absent:
/// the commit id makes the transport's integrity irrelevant, so there is
/// nothing plaintext buys that `https://` does not.
const SCHEMES: [&str; 5] = ["https://", "ssh://", "file://", "git@", "github:"];

/// `github:owner/repo`, spelled out. Sugar for the one host most roles
/// will live on; every other host is named by its URL.
const GITHUB: &str = "github:";

/// A role that lives in a git repository, pinned to one commit.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    /// Anything `git fetch` accepts. Never carries the pin.
    pub url: String,
    /// The commit to check out. Forty lowercase hex, always.
    pub sha: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum SourceError {
    Unpinned(String),
    NotARepo(String),
    UnusableName(String),
    BadVersion(String),
    Unreadable(String),
}

impl fmt::Display for SourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SourceError::Unpinned(text) => write!(
                f,
                "{text} names no commit; a remote role is pinned, never followed: \
                 add @<40 hex characters>"
            ),
            SourceError::NotARepo(text) => {
                write!(f, "{text} is not owner/repo")
            }
            SourceError::UnusableName(name) => write!(
                f,
                "{name:?} cannot name a role; names are letters, digits, - and _, \
                 so pass --as <name>"
            ),
            SourceError::BadVersion(claim) => {
                write!(f, "this wormhole reads source version 1, not {claim}")
            }
            SourceError::Unreadable(why) => write!(f, "source.toml is not valid: {why}"),
        }
    }
}

/// Whether `--role` was handed somewhere else rather than a local path or
/// an installed name. Asked first, because every remote form has a `/` in
/// it and the path rule would swallow them all.
fn is_remote_ref(text: &str) -> bool {
    SCHEMES.iter().any(|scheme| text.starts_with(scheme))
}

/// What a role reference names, before anything on disk is looked at.
///
/// Asked in this order because the tests overlap: every repository form
/// carries a `/`, so the path rule would swallow them all.
///
/// One body, so `--role` and `role add` can never read one string two
/// ways — which they did, with `/srv/mirror@<commit>` installing as a
/// repository and starting as a directory of that literal name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Names<'a> {
    /// A repository, pinned or not. An unpinned one is refused for having
    /// no commit rather than looked for on this disk.
    Repo(&'a str),
    /// A directory holding a role, wherever it is.
    Dir(&'a str),
    /// A role installed under the config home.
    Installed(&'a str),
}

pub fn names(text: &str) -> Names<'_> {
    // A repository may be named by a bare path — a local mirror, a share
    // — and then only the pin tells it from a folder of files.
    if is_remote_ref(text) || pin_of(text).is_some() {
        Names::Repo(text)
    } else if text.contains('/') {
        Names::Dir(text)
    } else {
        Names::Installed(text)
    }
}

/// The address and the commit a pinned ref splits into, or nothing.
///
/// Split off the *last* `@`, so an scp-style URL — which already contains
/// one — parses. A commit cannot contain `@`, so the last one is always
/// the separator and never part of the address.
fn pin_of(text: &str) -> Option<(&str, &str)> {
    text.rsplit_once('@').filter(|(_, sha)| is_sha(sha))
}

/// What names a role for as long as it is the same role, written so the
/// two kinds can never collide: a repository named by a bare path and a
/// directory of that same path are different roles, and an untagged
/// string would make them one.
pub fn dir_source(canonical: &std::path::Path) -> String {
    format!("{DIR}{}", canonical.display())
}

pub fn repo_source(url: &str) -> String {
    format!("{REPO}{url}")
}

const DIR: &str = "dir:";
const REPO: &str = "repo:";

/// Whether a string is one of the identities above.
pub fn is_source(text: &str) -> bool {
    text.starts_with(DIR) || text.starts_with(REPO)
}

/// A remote ref as the user types it: a URL or `github:owner/repo`, then
/// `@` and the commit.
///
/// The pin is split off the *last* `@`, so an scp-style URL — which
/// already contains one — parses. A commit cannot contain `@`, so the last
/// one is always the separator and never part of the address.
pub fn parse_ref(text: &str) -> Result<Source, SourceError> {
    let (address, sha) = pin_of(text).ok_or_else(|| SourceError::Unpinned(text.to_owned()))?;
    Ok(Source {
        url: expand(address)?,
        sha: sha.to_owned(),
    })
}

/// `github:owner/repo` to the URL it is short for; anything else is
/// already a URL and is passed through untouched.
fn expand(address: &str) -> Result<String, SourceError> {
    let Some(repo) = address.strip_prefix(GITHUB) else {
        return Ok(address.to_owned());
    };
    let ok = match repo.split_once('/') {
        Some((owner, name)) => {
            !owner.is_empty()
                && !name.is_empty()
                && !name.contains('/')
                && !repo.contains(char::is_whitespace)
        }
        None => false,
    };
    if !ok {
        return Err(SourceError::NotARepo(address.to_owned()));
    }
    Ok(format!("https://github.com/{repo}"))
}

/// What a commit looks like written down. Lowercase only, because the
/// checkout is named by this string and two spellings of one commit would
/// be two caches of one thing.
pub fn is_sha(text: &str) -> bool {
    crate::is_lowercase_hex(text, SHA_LEN)
}

/// What a role installed from this URL is called when the user names
/// nothing: the repository's own name. What the URL says, rather than a
/// convention the tool would then depend on.
pub fn default_name(url: &str) -> Result<String, SourceError> {
    let tail = url
        .rsplit(['/', ':'])
        .next()
        .unwrap_or_default()
        .trim_end_matches(".git");
    if !is_usable_name(tail) {
        return Err(SourceError::UnusableName(tail.to_owned()));
    }
    Ok(tail.to_owned())
}

/// Whether a string may name a role. A name becomes a directory under the
/// config home, so anything that could climb out of it, hide, or name the
/// current directory is refused rather than sanitised.
pub fn is_usable_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// The pointer file an installed remote role is: which repository, and
/// which commit of it. Written by `wormhole role add`, read on every
/// launch, and small enough to check by eye before trusting it.
pub const POINTER: &str = "source.toml";

pub fn to_toml(source: &Source) -> Result<String, String> {
    let text = toml::to_string(source).map_err(|e| format!("cannot serialize a source: {e}"))?;
    Ok(format!("version = 1\n{text}"))
}

pub fn parse(text: &str) -> Result<Source, SourceError> {
    let unreadable = |e: toml::de::Error| SourceError::Unreadable(e.message().to_owned());
    let mut table: toml::Table = toml::from_str(text).map_err(unreadable)?;
    // Taken out before the body is read, so a pointer from a version this
    // wormhole does not know says exactly that — rather than reporting
    // whatever that version renamed as an unknown field.
    let version = table
        .remove("version")
        .ok_or_else(|| SourceError::Unreadable("missing field `version`".to_owned()))?;
    if version != toml::Value::Integer(1) {
        return Err(SourceError::BadVersion(version.to_string()));
    }
    let source: Source = toml::Value::Table(table).try_into().map_err(unreadable)?;
    // A pointer is hand-editable, so the pin is checked here too. A `sha`
    // that is really a branch name would float a role that says it does
    // not.
    if !is_sha(&source.sha) {
        return Err(SourceError::Unpinned(source.sha));
    }
    Ok(source)
}

/// What a directory under `roles/` turns out to be.
///
/// Three callers ask this — the panel's listing, a launch resolving
/// `--role`, and `role add` deciding whether it may write here — and they
/// used to each answer it their own way, so a name that was not a role at
/// all produced a good message from one and a raw missing-file from
/// another.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoleKind {
    /// A pointer at the commit that holds the recipe.
    Pinned,
    /// The recipe itself, written by hand.
    Written,
    /// Neither. Not a role, whatever else the directory holds.
    Absent,
}

/// Which kind a directory is, from the two facts the binary can see.
///
/// A pointer wins over a manifest: `role add` refuses to write into a
/// directory that already holds a hand-written recipe, so the only way to
/// have both is to have put the manifest there afterwards — and the
/// pointer is still what wormhole was told to use.
pub fn role_kind(has_manifest: bool, has_pointer: bool) -> RoleKind {
    match (has_pointer, has_manifest) {
        (true, _) => RoleKind::Pinned,
        (false, true) => RoleKind::Written,
        (false, false) => RoleKind::Absent,
    }
}

/// What a pinned role still needs before a box can start from it.
///
/// A launch never fetches and never asks. Both are things a person is
/// present for, and `wormhole role add` is where that person is — so a
/// launch that finds either missing refuses and names the command,
/// instead of blocking a scripted start on a prompt nobody will see.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Missing {
    /// Fetched and approved. Start.
    Nothing,
    /// The pin names a checkout this host does not hold.
    Checkout,
    /// Fetched, but nobody has read what it runs and agreed to it.
    Approval,
}

/// Whether this pin is ready, from the two facts the binary can see.
///
/// A missing checkout is reported ahead of a missing approval because it
/// is the one to fix first: approving what is not there is not possible,
/// and one command fixes both.
pub fn missing(checkout: bool, approved: bool) -> Missing {
    match (checkout, approved) {
        (false, _) => Missing::Checkout,
        (true, false) => Missing::Approval,
        (true, true) => Missing::Nothing,
    }
}

impl Missing {
    /// The same three states as a listing column rather than a refusal.
    ///
    /// Beside the refusal on purpose: `role list` and a launch describe
    /// one fact, and two spellings of it would drift the moment either
    /// changed.
    pub fn state(self) -> &'static str {
        match self {
            Missing::Nothing => "ready",
            Missing::Checkout => "needs `role add` to fetch",
            Missing::Approval => "needs `role add` to approve",
        }
    }

    /// The refusal a launch prints, naming the command that fixes it.
    pub fn refusal(self, name: &str, source: &Source) -> Option<String> {
        if self == Missing::Nothing {
            return None;
        }
        let add = format!(
            "wormhole role add {}@{} --as {name}",
            source.url, source.sha
        );
        match self {
            Missing::Nothing => None,
            Missing::Checkout => Some(format!(
                "role {name} is pinned to {} of {}, which this host has not fetched; \
                 `{add}` fetches it",
                short(&source.sha),
                source.url
            )),
            Missing::Approval => Some(format!(
                "role {name} is fetched but not approved; \
                 `{add}` shows what it runs and asks"
            )),
        }
    }
}

/// A `--role <name>` that names nothing installed.
///
/// Looks next door before giving up: a bare word is never read as a path,
/// so a role sitting in the current folder under exactly that name is the
/// likeliest thing meant. `beside` is whether one is.
pub fn no_such_role(role: &str, roles_dir: &str, beside: bool) -> String {
    let mut said = format!("no role {role} in {roles_dir}");
    if beside {
        said.push_str(&format!(
            "\n  there is a role at ./{role} — start it with `--role ./{role}`, \
             or name it with `wormhole role add ./{role}`"
        ));
    } else {
        said.push_str("; `wormhole role add` installs one from a directory or a repository");
    }
    said
}

/// A pin as every surface names it: which repository, at which commit.
/// One body, so the approval screen, the `role list` row and the fetch
/// message cannot spell the same fact three ways.
pub fn describe(source: &Source) -> String {
    format!("{} at {}", source.url, short(&source.sha))
}

/// A commit as a person reads it back, for a message rather than a path.
/// Clamped rather than sliced, so a message can never be the thing that
/// panics.
pub fn short(sha: &str) -> &str {
    &sha[..12.min(sha.len())]
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA: &str = "1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e";

    #[test]
    fn a_url_and_a_commit_are_what_a_remote_role_is() {
        assert_eq!(
            parse_ref(&format!("https://github.com/you/alphaca-java@{SHA}")),
            Ok(Source {
                url: "https://github.com/you/alphaca-java".to_owned(),
                sha: SHA.to_owned(),
            })
        );
    }

    /// `github:` is the only shorthand, and it is only a shorthand: it
    /// expands to the URL and nothing downstream knows it existed.
    #[test]
    fn the_github_shorthand_expands_to_the_url_it_stands_for() {
        assert_eq!(
            parse_ref(&format!("github:you/alphaca-java@{SHA}")).map(|s| s.url),
            Ok("https://github.com/you/alphaca-java".to_owned())
        );
    }

    /// An scp-style URL already carries an `@`. Splitting on the first one
    /// would take the address apart; the commit cannot contain `@`, so the
    /// last one is always the separator.
    #[test]
    fn the_pin_is_split_off_the_last_at_so_an_scp_url_survives() {
        assert_eq!(
            parse_ref(&format!("git@github.com:you/role.git@{SHA}")),
            Ok(Source {
                url: "git@github.com:you/role.git".to_owned(),
                sha: SHA.to_owned(),
            })
        );
    }

    /// The whole reason a remote role is safe to run twice: a commit
    /// cannot change under you, and a branch can. An unpinned ref is
    /// refused at parse time rather than resolved against whatever the
    /// server is serving today.
    #[test]
    fn a_ref_that_names_no_commit_is_refused_not_floated() {
        for unpinned in [
            "github:you/role",
            "https://github.com/you/role",
            "git@github.com:you/role.git",
            // A branch where a commit belongs.
            "github:you/role@main",
            // Short: ambiguous locally, refused by the server anyway.
            "github:you/role@1b2c3d4",
        ] {
            assert!(
                matches!(parse_ref(unpinned), Err(SourceError::Unpinned(_))),
                "{unpinned}"
            );
        }
    }

    /// One spelling of a commit, because the checkout is named by this
    /// string: two spellings would be two caches of one thing, and two
    /// approvals for one set of bytes.
    #[test]
    fn a_commit_is_lowercase_hex_at_full_length() {
        assert!(is_sha(SHA));
        assert!(!is_sha(&SHA.to_uppercase()));
        assert!(!is_sha(&SHA[..39]));
        assert!(!is_sha(&format!("{SHA}0")));
        assert!(!is_sha(""));
        assert!(!is_sha(&"g".repeat(40)));
    }

    #[test]
    fn the_shorthand_wants_exactly_owner_and_repo() {
        for bad in [
            "github:role",
            "github:/role",
            "github:you/",
            "github:you/roles/inner",
            "github:you /role",
        ] {
            assert!(
                matches!(
                    parse_ref(&format!("{bad}@{SHA}")),
                    Err(SourceError::NotARepo(_))
                ),
                "{bad}"
            );
        }
    }

    /// Every remote form contains a `/`, so `--role` cannot reach its
    /// path rule before asking this.
    #[test]
    fn a_remote_ref_is_told_apart_before_anything_looks_like_a_path() {
        for remote in [
            "https://github.com/you/role",
            "ssh://git@host/you/role",
            "file:///srv/mirrors/role",
            "git@github.com:you/role.git",
            "github:you/role",
        ] {
            assert!(is_remote_ref(remote), "{remote}");
        }
        // A bare path is a directory on this machine, and stays one. Only
        // a named transport turns it into something to fetch.
        for local in ["alphaca", "./roles/alphaca", "/abs/role", "../up"] {
            assert!(!is_remote_ref(local), "{local}");
        }
    }

    /// One rule about what a reference names, so no two commands can read
    /// one string two ways. The pin is what tells a bare path apart from a
    /// folder of files.
    #[test]
    fn a_reference_names_a_repository_a_directory_or_an_installed_name() {
        for repo in [
            format!("/srv/mirrors/role@{SHA}"),
            format!("github:you/role@{SHA}"),
            // A transport is a repository whether or not it is pinned: an
            // unpinned one is refused for having no commit, not looked
            // for on this disk.
            "github:you/role".to_owned(),
            "https://host/you/role".to_owned(),
        ] {
            assert_eq!(names(&repo), Names::Repo(&repo), "{repo}");
        }
        for dir in [
            "./roles/alphaca",
            "/home/me/roles/alphaca",
            // A directory whose name happens to contain an `@`.
            "./roles/user@host",
        ] {
            assert_eq!(names(dir), Names::Dir(dir), "{dir}");
        }
        for installed in ["alphaca", "alphaca-java", "r2"] {
            assert_eq!(names(installed), Names::Installed(installed), "{installed}");
        }
    }

    /// A repository named by a bare path and a directory of that same path
    /// are two roles. Untagged, their identities would be one string and
    /// the two would share a box.
    #[test]
    fn the_two_kinds_of_source_cannot_collide() {
        let path = std::path::Path::new("/srv/roles/alphaca");
        assert_ne!(dir_source(path), repo_source("/srv/roles/alphaca"));
        assert_eq!(dir_source(path), "dir:/srv/roles/alphaca");
        assert_eq!(repo_source("/srv/roles/alphaca"), "repo:/srv/roles/alphaca");
    }

    /// A bare word is never a path, so the refusal for a name that is not
    /// installed has to name the directory one character away — otherwise
    /// it points only at the config folder the role is not in.
    #[test]
    fn a_name_that_misses_points_at_the_role_beside_it() {
        let beside = no_such_role("alphaca", "/c/roles", true);
        assert!(beside.contains("./alphaca"), "{beside}");
        assert!(beside.contains("role add"), "{beside}");

        let nothing = no_such_role("alphaca", "/c/roles", false);
        assert!(nothing.contains("/c/roles"), "{nothing}");
        assert!(!nothing.contains("./alphaca"), "{nothing}");
        assert!(nothing.contains("role add"), "{nothing}");
    }

    #[test]
    fn a_pin_is_described_the_same_way_wherever_it_is_shown() {
        let source = Source {
            url: "https://github.com/you/role".to_owned(),
            sha: SHA.to_owned(),
        };
        assert_eq!(
            describe(&source),
            format!("https://github.com/you/role at {}", short(SHA))
        );
    }

    #[test]
    fn a_role_is_named_by_the_repository_it_came_from() {
        for (url, name) in [
            ("https://github.com/you/alphaca-java", "alphaca-java"),
            ("https://github.com/you/alphaca-java.git", "alphaca-java"),
            ("git@github.com:you/role.git", "role"),
            ("ssh://git@host/a/b/deep_role", "deep_role"),
        ] {
            assert_eq!(default_name(url).as_deref(), Ok(name), "{url}");
        }
    }

    /// A name becomes a directory under the config home. One that could
    /// climb out of it, hide in it, or name it is refused rather than
    /// quietly repaired into something else.
    #[test]
    fn a_name_that_could_escape_the_config_directory_is_refused() {
        for bad in ["", ".", "..", ".hidden", "a/b", "a b", "a:b", "a\0b"] {
            assert!(!is_usable_name(bad), "{bad:?}");
        }
        for good in ["alphaca", "alphaca-java", "geohod_engineer", "r2"] {
            assert!(is_usable_name(good), "{good}");
        }
        assert!(matches!(
            default_name("https://host/you/.git"),
            Err(SourceError::UnusableName(_))
        ));
    }

    #[test]
    fn a_pointer_survives_the_round_trip() {
        let source = Source {
            url: "https://github.com/you/role".to_owned(),
            sha: SHA.to_owned(),
        };
        let text = to_toml(&source).expect("serializes");
        assert!(text.contains("version = 1"), "{text}");
        assert_eq!(parse(&text), Ok(source));
    }

    /// The pointer is a file a person can edit, so the pin is checked on
    /// the way in too. A branch name written where a commit belongs would
    /// otherwise float a role whose whole promise is that it does not.
    #[test]
    fn a_hand_edited_pointer_that_floats_is_refused() {
        let text = "version = 1\nurl = \"https://h/r\"\nsha = \"main\"\n";
        assert!(matches!(parse(text), Err(SourceError::Unpinned(_))));
    }

    #[test]
    fn a_pointer_from_another_version_says_so_instead_of_guessing() {
        let text = format!("version = 2\nurl = \"https://h/r\"\nsha = \"{SHA}\"\n");
        assert!(matches!(parse(&text), Err(SourceError::BadVersion(_))));
    }

    #[test]
    fn an_unreadable_pointer_is_an_error_not_a_panic() {
        for bad in ["", "{", "url = \"https://h/r\"\n", "version = 1\n"] {
            assert!(parse(bad).is_err(), "{bad:?}");
        }
    }

    /// One answer to "what is this directory", so the panel that lists a
    /// role, the launch that resolves it and the `role add` that writes it
    /// can never disagree about what they are looking at.
    #[test]
    fn a_role_directory_is_a_pointer_a_recipe_or_not_a_role() {
        assert_eq!(role_kind(false, true), RoleKind::Pinned);
        assert_eq!(role_kind(true, false), RoleKind::Written);
        assert_eq!(role_kind(false, false), RoleKind::Absent);
        // Both is not a state `role add` can produce — it refuses to write
        // over a hand-written recipe — so the pointer, which is what
        // wormhole was told to use, decides.
        assert_eq!(role_kind(true, true), RoleKind::Pinned);
    }

    /// A launch fetches nothing and asks nothing. Both facts must hold
    /// before a box starts, and either one missing is a refusal that names
    /// the command a person runs.
    #[test]
    fn a_pin_is_ready_only_when_it_is_both_fetched_and_approved() {
        assert_eq!(missing(true, true), Missing::Nothing);
        assert_eq!(missing(true, false), Missing::Approval);
        assert_eq!(missing(false, false), Missing::Checkout);
        // Approved with nothing fetched is still nothing to run, and the
        // fetch is what a person fixes first.
        assert_eq!(missing(false, true), Missing::Checkout);
    }

    /// Every state a listing can show says what it needs, and only the
    /// ready one says nothing has to happen.
    #[test]
    fn every_state_a_listing_shows_names_what_it_still_needs() {
        assert_eq!(Missing::Nothing.state(), "ready");
        for missing in [Missing::Checkout, Missing::Approval] {
            assert!(missing.state().contains("role add"), "{missing:?}");
        }
    }

    #[test]
    fn a_refusal_names_the_command_that_fixes_it() {
        let source = Source {
            url: "https://github.com/you/role".to_owned(),
            sha: SHA.to_owned(),
        };
        assert_eq!(Missing::Nothing.refusal("role", &source), None);
        for missing in [Missing::Checkout, Missing::Approval] {
            let refusal = missing.refusal("role", &source).expect("a refusal");
            assert!(refusal.contains("wormhole role add"), "{refusal}");
            assert!(refusal.contains(SHA), "{refusal}");
            assert!(refusal.contains("--as role"), "{refusal}");
        }
    }
}
