//! The box's trust store: where a trusted host bundle lands, and what has
//! to be said for the programs in the box to actually read it.
//!
//! Mounting the bundle is only half the job. Most TLS clients do not look
//! at that path unless they are told to, and several ignore the generic
//! `SSL_CERT_FILE` outright — measured on the shipped Alpine image, `git`
//! ignores `SSL_CERT_FILE`, `SSL_CERT_DIR` *and* `CURL_CA_BUNDLE`, because
//! its libcurl carries a compiled-in bundle path. Node, which the agent
//! itself is, carries its own list of roots and never reads the filesystem
//! at all.
//!
//! So `host_ca = true` cannot mean "the box trusts what the host trusts"
//! by mounting a file. It has to name that file to every client that will
//! listen.

/// Where a trusted host bundle is bound inside the box.
///
/// The path a fresh Alpine image already uses, so a client with a
/// compiled-in default finds it without being told.
pub const CA_BUNDLE_IN_BOX: &str = "/etc/ssl/certs/ca-certificates.crt";

/// Where a trusted host bundle is bound inside a build box.
///
/// Not the canonical path the run box uses: `apk add ca-certificates`
/// writes `/etc/ssl/certs/ca-certificates.crt` while the build is
/// running — the file belongs to `ca-certificates-bundle` — so a
/// read-only bind over it breaks the install that needs it.
///
/// And not the build box's scratch either, which is where artifacts land:
/// a recipe naming this path as an `into` would be handed the host's
/// bundle where a digest promised its own bytes. Two tmpfs, two
/// namespaces, and the collision cannot be written.
///
/// OpenSSL reads `SSL_CERT_DIR` as well as `SSL_CERT_FILE` and trusts the
/// union, so naming this file adds the host's roots to the image's own
/// rather than replacing them.
pub const CA_BUNDLE_IN_BUILD: &str = "/run/wormhole-ca.crt";

/// What each client reads to find a bundle, when it will not take the
/// path on its own.
///
/// Every one of these is a variable that some client honours and another
/// ignores; there is no single lever. They are defaults rather than
/// overrides — a manifest that declares one of these names itself means
/// it, and wins.
pub const READERS: [&str; 6] = [
    // Node, and so the agent itself. Node ships a snapshot of the Mozilla
    // store fixed at release time and does not read the OS store at all;
    // installing a CA on the host is invisible to it. This one appends,
    // where `--use-openssl-ca` would *replace* Node's roots with the
    // host's — and the host bundle is not always a superset.
    "NODE_EXTRA_CA_CERTS",
    // git sets libcurl's CA path only from these; libcurl itself reads no
    // environment at all, so `CURL_CA_BUNDLE` and `SSL_CERT_FILE` both
    // have no effect on git.
    "GIT_SSL_CAINFO",
    "CARGO_HTTP_CAINFO",
    // Honoured by OpenSSL, Go, Python's `ssl` and `rustls-native-certs`.
    // Its companion `SSL_CERT_DIR` is deliberately absent: OpenSSL trusts
    // the union of the two, and its default is the image's own
    // `/etc/ssl/certs`, so leaving it alone is what makes this an addition
    // to the box's trust rather than a replacement of it.
    "SSL_CERT_FILE",
    // Read by the `curl` binary, never by libcurl.
    "CURL_CA_BUNDLE",
    // Python's requests, which uses certifi rather than the OS store and
    // does not look at `SSL_CERT_FILE`.
    "REQUESTS_CA_BUNDLE",
];

/// Every reader named, pointed at one bundle.
///
/// One body, because the running box and the build box both have to do
/// this and a second copy is how the two drift apart on which clients get
/// told. What each caller decides is only whether a value already there
/// wins.
pub fn readers_pointing_at(bundle: &str) -> impl Iterator<Item = (&'static str, String)> {
    READERS.into_iter().map(|name| (name, bundle.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The one the agent itself needs. Node reads no trust store from the
    /// filesystem, so without this `host_ca = true` mounts a bundle that
    /// the thing wormhole exists to run never opens.
    #[test]
    fn the_agents_own_runtime_is_among_them() {
        assert!(READERS.contains(&"NODE_EXTRA_CA_CERTS"), "{READERS:?}");
    }

    /// The bug this pins: with the bundle on the build box's scratch, a
    /// recipe could name `into = "/tmp/wormhole-ca.crt"` and be handed the
    /// host's certificates where its own digest-proved bytes were
    /// promised — the "digest says something untrue" failure the artifact
    /// exists to prevent. An artifact may only land on the scratch, so
    /// keeping the bundle off it is what makes that unwritable.
    #[test]
    fn the_build_bundle_is_not_where_an_artifact_could_land() {
        assert!(
            !std::path::Path::new(CA_BUNDLE_IN_BUILD).starts_with(crate::mount_plan::BUILD_SCRATCH),
            "{CA_BUNDLE_IN_BUILD} is somewhere an artifact could be asked for"
        );
    }

    /// The run box's bundle is the path a client with a compiled-in
    /// default already looks at; the build box's must not be, or
    /// `apk add ca-certificates` cannot write the file it owns.
    #[test]
    fn the_two_bundles_never_share_a_path() {
        assert_ne!(CA_BUNDLE_IN_BOX, CA_BUNDLE_IN_BUILD);
    }

    #[test]
    fn no_reader_is_named_twice() {
        let mut names = READERS.to_vec();
        names.sort_unstable();
        let before = names.len();
        names.dedup();
        assert_eq!(names.len(), before, "a reader is named twice");
    }
}
