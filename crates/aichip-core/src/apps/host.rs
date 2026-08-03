//! Which hostname is which.
//!
//! Two suffixes reach the proxy and they must never be confused, because they
//! carry different privileges: a `.preview.localhost` name is a branch under
//! review and gets *nothing*, while a `.app.localhost` name is an installed app
//! and may hold grants. Reading one as the other would hand a card's worktree
//! whatever its project's app was allowed to do.
//!
//! The match is exact-suffix and single-label, the same rule `preview_proxy`
//! has always used, and the confusion matrix is pinned below rather than left
//! to be reasoned about at each call site.

pub const PREVIEW_SUFFIX: &str = ".preview.localhost";
pub const APP_SUFFIX: &str = ".app.localhost";

/// Names an app may not take, because aichip answers to them itself.
///
/// `probe` is the load-bearing one: the dashboard asks for it to find out
/// whether this browser resolves `*.localhost` at all, and it has to get an
/// answer whether or not any app exists. Without it, "your browser cannot
/// resolve this" and "that container is down" look identical.
pub const RESERVED: [&str; 5] = ["probe", "health", "api", "www", "admin"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostKind {
    /// A card's branch, running in a container. No capabilities, ever.
    Preview,
    /// An installed app. May hold grants.
    App,
}

/// Classify a `Host` header.
///
/// `None` for everything that is not one of ours — including the dashboard's
/// own names, which must fall through rather than be read as a slug.
pub fn classify(host: &str) -> Option<(HostKind, &str)> {
    let bare = bare(host);
    // App first, but the order does not matter: a name cannot end with both
    // suffixes, and `label` rejects anything left with a dot in it.
    if let Some(l) = bare.strip_suffix(APP_SUFFIX) {
        return label(l).map(|l| (HostKind::App, l));
    }
    if let Some(l) = bare.strip_suffix(PREVIEW_SUFFIX) {
        return label(l).map(|l| (HostKind::Preview, l));
    }
    None
}

/// One label, and a real one.
///
/// A dot here means a longer name that merely *ends* with our suffix:
/// `x.app.localhost.attacker.com` cannot reach this at all because the suffix
/// is not at the end, but `a.b.app.localhost` would, and it is not ours either.
/// It is also what keeps `x.preview.app.localhost` from being read as a preview.
fn label(l: &str) -> Option<&str> {
    if l.is_empty() || l.contains('.') {
        None
    } else {
        Some(l)
    }
}

/// The host part of a `Host` or `Origin`, without scheme, path or port.
fn bare(value: &str) -> &str {
    let authority = value.rsplit_once("://").map_or(value, |(_, rest)| rest);
    let authority = authority.split('/').next().unwrap_or("");
    match authority.strip_prefix('[') {
        Some(rest) => match rest.split_once(']') {
            Some((inner, _)) => &authority[..(inner.len() + 2).min(authority.len())],
            None => authority,
        },
        None => authority.split(':').next().unwrap_or(""),
    }
}

/// Whether a path is inside the reserved capability prefix.
///
/// Split into segments and compared exactly, never `starts_with`: hyper does
/// not normalise a URI, so `/__aichip/../api` reaches the router with its dots
/// intact and a prefix test would let it through to be resolved later by
/// something that does normalise.
pub const BRIDGE_PREFIX: &str = "__aichip";

/// The bridge sub-path, when this request is for the bridge at all.
///
/// `Some(rest)` for a bridge request — `rest` never contains a `.` or `..`
/// segment, because such a path is refused outright rather than cleaned up.
/// There is no legitimate bridge path with a dot segment in it, so rejecting is
/// both safe and simpler than normalising.
pub fn bridge_path(path: &str) -> Option<Result<Vec<&str>, Traversal>> {
    let mut segments = path.split('/').filter(|s| !s.is_empty());
    if segments.next() != Some(BRIDGE_PREFIX) {
        return None;
    }
    let rest: Vec<&str> = segments.collect();
    if rest.iter().any(|s| *s == "." || *s == "..") {
        return Some(Err(Traversal));
    }
    Some(Ok(rest))
}

/// A bridge path containing a `.` or `..` segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Traversal;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_suffix_is_read_as_itself() {
        assert_eq!(classify("notes-a1b2c3.app.localhost"), Some((HostKind::App, "notes-a1b2c3")));
        assert_eq!(
            classify("card-a.preview.localhost:4820"),
            Some((HostKind::Preview, "card-a"))
        );
        assert_eq!(classify("http://x.app.localhost:4820/"), Some((HostKind::App, "x")));
    }

    #[test]
    fn the_two_suffixes_cannot_be_confused_with_each_other() {
        // The attack worth naming: a preview whose label makes the name look
        // like an app's, or the other way round. Both strip to a label with a
        // dot in it, which is not a label.
        assert_eq!(classify("x.app.preview.localhost"), None);
        assert_eq!(classify("x.preview.app.localhost"), None);
    }

    #[test]
    fn a_name_that_merely_contains_a_suffix_is_not_ours() {
        assert_eq!(classify("x.app.localhost.attacker.com"), None);
        assert_eq!(classify("app.localhost.evil.test"), None);
        assert_eq!(classify("notapp.localhost"), None);
        // Nested labels are not ours either.
        assert_eq!(classify("a.b.app.localhost"), None);
        assert_eq!(classify("a.b.preview.localhost"), None);
        // An empty label is not a name.
        assert_eq!(classify(".app.localhost"), None);
        assert_eq!(classify("app.localhost"), None);
    }

    #[test]
    fn the_dashboards_own_hosts_fall_through() {
        // They must reach the dashboard router, not be read as a slug.
        for host in ["localhost:4820", "127.0.0.1:4820", "[::1]:4820", "localhost"] {
            assert_eq!(classify(host), None, "{host} was taken for a slug");
        }
    }

    #[test]
    fn ipv6_keeps_its_brackets_and_loses_its_port() {
        assert_eq!(bare("[::1]:4820"), "[::1]");
        assert_eq!(bare("http://[::1]:4820"), "[::1]");
    }

    #[test]
    fn the_bridge_prefix_is_matched_by_segment_not_by_prefix() {
        assert_eq!(bridge_path("/__aichip/me"), Some(Ok(vec!["me"])));
        assert_eq!(bridge_path("/__aichip/kv/a"), Some(Ok(vec!["kv", "a"])));
        assert_eq!(bridge_path("/__aichip"), Some(Ok(vec![])));
        // Not the bridge at all.
        assert_eq!(bridge_path("/index.html"), None);
        assert_eq!(bridge_path("/"), None);
        // A path that merely starts with the same letters is not the bridge —
        // which a `starts_with` test would have got wrong.
        assert_eq!(bridge_path("/__aichipsomething/me"), None);
    }

    #[test]
    fn a_dot_segment_is_refused_rather_than_cleaned_up() {
        // hyper does not normalise a URI, so these arrive intact. Refusing is
        // safer than normalising, because there is no legitimate bridge path
        // with a dot segment in it and nothing then has to agree about how the
        // cleaning was done.
        assert_eq!(bridge_path("/__aichip/../api/settings"), Some(Err(Traversal)));
        assert_eq!(bridge_path("/__aichip/data/../../settings"), Some(Err(Traversal)));
        assert_eq!(bridge_path("/__aichip/./me"), Some(Err(Traversal)));
    }

    #[test]
    fn the_reserved_names_include_the_one_the_probe_needs() {
        // Without `probe`, "your browser will not resolve *.localhost" and
        // "that container is down" are the same blank page.
        assert!(RESERVED.contains(&"probe"));
    }
}
