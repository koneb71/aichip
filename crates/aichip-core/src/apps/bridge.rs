//! Everything a container app can ask aichip for.
//!
//! One enum. If a route is not a variant here it does not exist, and adding a
//! variant without saying which scope it needs does not compile — `scope()` is
//! exhaustive with no wildcard arm, deliberately, because a pattern allowlist
//! over a router that grows is a bypass waiting to be added by someone who does
//! not know the allowlist is there.
//!
//! ## Why not proxy `/api`
//!
//! Because the response *shapes* have to differ. `GET /api/tasks` returns
//! `t.prompt` — text a person typed, which may contain anything — and an app
//! with `read:board` has no business seeing it. A proxy that forwarded the
//! dashboard's own response would hand over fields nobody decided to share.
//! Every variant below names what it returns.
//!
//! ## How a request proves which app it is
//!
//! By its hostname, which it cannot forge: the request never reaches the
//! container, so there is nothing in the path between the browser and this
//! module that the app controls. See `preview_proxy`.

use super::scope::Scope;

/// The reserved paths, resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    /// The client library. The one path exempt from the header gate — a static
    /// script with no data in it and no side effect, which has to load before
    /// anything can send the header it would otherwise need.
    ClientJs,
    /// Answers whether this hostname resolves at all. Ungated, and also served
    /// on the reserved `probe` slug where no app exists.
    Health,
    /// Who this app is and what it holds. Never anything about the workspace.
    Me,

    /// The app's own tables. No scope: they exist because it declared them,
    /// hold only what it put there, and are dropped with it.
    Schema,
    DataList(String),
    DataGet(String, String),
    DataCreate(String),
    DataUpdate(String, String),
    DataDelete(String, String),

    /// aichip's data, behind a grant.
    Projects,
    Tasks,
    Runs,
    Spend,
    Agents,
    KbPages,
    CreateTask,

    /// Anything else. The only place that decides a path does not exist.
    Unknown,
    /// A known path asked for with the wrong verb, kept apart from Unknown so
    /// it can 405 — "you used the wrong method" is a fix, "no such thing" is a
    /// hunt.
    WrongMethod,
}

impl Route {
    /// What a person must have granted before this may be served.
    ///
    /// Exhaustive on purpose. A new variant added without an arm here is a
    /// compile error, which is the only way an allowlist stays one.
    pub fn scope(&self) -> Option<Scope> {
        match self {
            Route::ClientJs
            | Route::Health
            | Route::Me
            | Route::Schema
            | Route::DataList(_)
            | Route::DataGet(_, _)
            | Route::DataCreate(_)
            | Route::DataUpdate(_, _)
            | Route::DataDelete(_, _)
            | Route::Unknown
            | Route::WrongMethod => None,

            Route::Projects => Some(Scope::ReadProjects),
            Route::Tasks => Some(Scope::ReadBoard),
            Route::Runs => Some(Scope::ReadRuns),
            Route::Spend => Some(Scope::ReadSpend),
            Route::Agents => Some(Scope::ReadAgents),
            Route::KbPages => Some(Scope::ReadKb),
            Route::CreateTask => Some(Scope::WriteBoard),
        }
    }

    /// Whether this may be served without the `X-Aichip-App` header.
    ///
    /// Exactly one path, and it has to be: the client library is what sets the
    /// header, so requiring the header to fetch it would be a loop nothing can
    /// enter. It carries no data and does nothing, which is what makes the
    /// exception safe rather than merely necessary.
    pub fn header_exempt(&self) -> bool {
        matches!(self, Route::ClientJs | Route::Health)
    }
}

/// Resolve a bridge path.
///
/// `segments` is what [`super::host::bridge_path`] returned — already checked
/// for dot segments, so nothing here has to think about traversal.
///
/// The method is a `&str` rather than an `http::Method` so this crate stays
/// unaware of the web framework: the routing table is a fact about the
/// capability surface, not about axum.
pub fn route(method: &str, segments: &[&str]) -> Route {
    let get = method == "GET";
    let post = method == "POST";
    let patch = method == "PATCH";
    let delete = method == "DELETE";

    // A helper for the many "right path, wrong verb" cases.
    let only = |ok: bool, route: Route| if ok { route } else { Route::WrongMethod };

    match segments {
        ["client.js"] => only(get, Route::ClientJs),
        ["health"] => only(get, Route::Health),
        ["me"] => only(get, Route::Me),
        ["schema"] => only(get, Route::Schema),

        ["data", model] => {
            if get {
                Route::DataList(model.to_string())
            } else if post {
                Route::DataCreate(model.to_string())
            } else {
                Route::WrongMethod
            }
        }
        ["data", model, row] => {
            if get {
                Route::DataGet(model.to_string(), row.to_string())
            } else if patch {
                Route::DataUpdate(model.to_string(), row.to_string())
            } else if delete {
                Route::DataDelete(model.to_string(), row.to_string())
            } else {
                Route::WrongMethod
            }
        }

        ["api", "projects"] => only(get, Route::Projects),
        ["api", "tasks"] => {
            if get {
                Route::Tasks
            } else if post {
                Route::CreateTask
            } else {
                Route::WrongMethod
            }
        }
        ["api", "runs"] => only(get, Route::Runs),
        ["api", "spend"] => only(get, Route::Spend),
        ["api", "agents"] => only(get, Route::Agents),
        ["api", "kb", "pages"] => only(get, Route::KbPages),

        _ => Route::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(method: &str, path: &str) -> Route {
        let segments = super::super::host::bridge_path(path)
            .expect("a bridge path")
            .expect("no dot segments");
        route(method, &segments)
    }

    #[test]
    fn every_allowlisted_pair_resolves() {
        assert_eq!(r("GET", "/__aichip/client.js"), Route::ClientJs);
        assert_eq!(r("GET", "/__aichip/me"), Route::Me);
        assert_eq!(r("GET", "/__aichip/schema"), Route::Schema);
        assert_eq!(r("GET", "/__aichip/data/note"), Route::DataList("note".into()));
        assert_eq!(r("POST", "/__aichip/data/note"), Route::DataCreate("note".into()));
        assert_eq!(
            r("PATCH", "/__aichip/data/note/abc"),
            Route::DataUpdate("note".into(), "abc".into())
        );
        assert_eq!(r("GET", "/__aichip/api/projects"), Route::Projects);
        assert_eq!(r("POST", "/__aichip/api/tasks"), Route::CreateTask);
    }

    #[test]
    fn nothing_outside_the_table_exists() {
        // The whole surface is the enum. Anything else is Unknown, including
        // paths that look plausible.
        for path in [
            "/__aichip/api/settings",
            "/__aichip/api/settings/models",
            "/__aichip/api/fs/list",
            "/__aichip/api/attachments",
            "/__aichip/api/kb/assets/1",
            "/__aichip/api/workspaces",
            "/__aichip/api",
            "/__aichip",
            "/__aichip/data",
        ] {
            assert_eq!(r("GET", path), Route::Unknown, "{path} resolved");
        }
    }

    #[test]
    fn an_app_can_file_a_card_but_never_start_a_run() {
        // Starting a run spends money and executes code. It is not reachable
        // from the bridge at all — not behind a scope, absent.
        for path in ["/__aichip/api/tasks/abc/start", "/__aichip/api/runs/abc/cancel"] {
            assert_eq!(r("POST", path), Route::Unknown, "{path} resolved");
        }
        assert_eq!(Route::CreateTask.scope(), Some(Scope::WriteBoard));
    }

    #[test]
    fn the_wrong_verb_is_distinguishable_from_the_wrong_path() {
        assert_eq!(r("DELETE", "/__aichip/me"), Route::WrongMethod);
        assert_eq!(r("POST", "/__aichip/api/projects"), Route::WrongMethod);
        assert_eq!(r("PUT", "/__aichip/data/note"), Route::WrongMethod);
        // …and neither is served.
        assert_eq!(Route::WrongMethod.scope(), None);
        assert_eq!(Route::Unknown.scope(), None);
    }

    #[test]
    fn an_apps_own_tables_need_no_grant_and_aichips_data_always_does() {
        for route in [
            Route::Schema,
            Route::DataList("x".into()),
            Route::DataCreate("x".into()),
            Route::DataDelete("x".into(), "y".into()),
        ] {
            assert_eq!(route.scope(), None, "{route:?} should need nothing");
        }
        for route in [
            Route::Projects,
            Route::Tasks,
            Route::Runs,
            Route::Spend,
            Route::Agents,
            Route::KbPages,
            Route::CreateTask,
        ] {
            assert!(route.scope().is_some(), "{route:?} must be gated");
        }
    }

    #[test]
    fn only_the_client_library_and_the_probe_skip_the_header() {
        // The library is what sets the header, so needing it to fetch the
        // library would be a loop nothing can enter. Everything else pays.
        assert!(Route::ClientJs.header_exempt());
        assert!(Route::Health.header_exempt());
        for route in [
            Route::Me,
            Route::Schema,
            Route::DataList("x".into()),
            Route::Projects,
            Route::CreateTask,
            Route::Unknown,
        ] {
            assert!(!route.header_exempt(), "{route:?} must carry the header");
        }
    }

    #[test]
    fn a_model_name_is_carried_not_interpreted() {
        // Whatever is in the path reaches `apps::data`, which looks it up among
        // the declared models and refuses anything else. This layer does not
        // get to decide a table exists.
        assert_eq!(
            r("GET", "/__aichip/data/'; DROP TABLE x; --"),
            Route::DataList("'; DROP TABLE x; --".into())
        );
    }
}
