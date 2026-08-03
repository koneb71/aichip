//! The client library aichip serves to its own apps.
//!
//! Served rather than written by whoever generated the app, because three
//! things have to be right on every single call and none of them is
//! interesting: the `X-Aichip-App` header that makes cross-origin requests
//! fail at the preflight, the shape of an error, and what to do when a scope is
//! missing. Left to an agent, each of those would be wrong in a different way
//! in every app.
//!
//! It is the one path exempt from the header gate — it is what *sets* the
//! header, so needing it would be a loop nothing can enter. That is safe rather
//! than merely necessary: it carries no data and has no side effect.

/// The names `window.aichip` exposes.
///
/// Kept as data so a test can check the scaffold prompt still describes the
/// same API. Two descriptions of one interface drift, and the symptom is an
/// app calling a method that does not exist.
pub const METHODS: [&str; 8] = [
    "me",
    "schema",
    "list",
    "get",
    "create",
    "update",
    "remove",
    "api",
];

pub const CLIENT_JS: &str = r#"// aichip app client. Served by aichip; do not vendor a copy.
//
// Every call carries X-Aichip-App, which is what makes this API unreachable
// from any other origin: a custom header forces a preflight, and aichip answers
// no CORS at all, so the real request is never sent.
(function () {
  var BASE = "/__aichip";

  function request(method, path, body) {
    var init = {
      method: method,
      headers: { "X-Aichip-App": "1" },
      // Same-origin only. This API does not exist anywhere else.
      credentials: "same-origin",
    };
    if (body !== undefined) {
      init.headers["Content-Type"] = "application/json";
      init.body = JSON.stringify(body);
    }
    return fetch(BASE + path, init).then(function (res) {
      return res.text().then(function (text) {
        var data = null;
        try { data = text ? JSON.parse(text) : null; } catch (e) { data = null; }
        if (res.ok) return data;

        // A missing permission is not a failure of the app — it is something
        // the person can grant. Given its own error name so an app can say so
        // rather than showing a stack trace.
        if (res.status === 403 && data && data.needsScope) {
          var err = new Error(
            "This app needs the \"" + data.needsScope + "\" permission. " +
            "Grant it under Permissions in aichip."
          );
          err.name = "AichipNeedsScope";
          err.scope = data.needsScope;
          throw err;
        }
        var message = (data && (data.error || data.message)) || text || res.statusText;
        var failure = new Error(message);
        failure.name = "AichipError";
        failure.status = res.status;
        throw failure;
      });
    });
  }

  function query(params) {
    if (!params) return "";
    var parts = [];
    Object.keys(params).forEach(function (key) {
      var value = params[key];
      if (value === undefined || value === null) return;
      // `where` repeats. An object would keep only the last one and quietly
      // return the wrong rows.
      if (Array.isArray(value)) {
        value.forEach(function (v) {
          parts.push(encodeURIComponent(key) + "=" + encodeURIComponent(v));
        });
      } else {
        parts.push(encodeURIComponent(key) + "=" + encodeURIComponent(value));
      }
    });
    return parts.length ? "?" + parts.join("&") : "";
  }

  window.aichip = {
    /** Who this app is, and which permissions it holds. */
    me: function () { return request("GET", "/me"); },

    /** The models this app declared, as aichip understands them. */
    schema: function () { return request("GET", "/schema"); },

    /**
     * Rows of one of your own models.
     *
     * `opts.where` is a list of "field:op:value" — never SQL. Operators are
     * eq, ne, gt, gte, lt, lte, like, in, isnull, notnull.
     */
    list: function (model, opts) {
      return request("GET", "/data/" + encodeURIComponent(model) + query(opts));
    },
    get: function (model, id) {
      return request("GET", "/data/" + encodeURIComponent(model) + "/" + encodeURIComponent(id));
    },
    create: function (model, values) {
      return request("POST", "/data/" + encodeURIComponent(model), values);
    },
    update: function (model, id, values) {
      return request(
        "PATCH", "/data/" + encodeURIComponent(model) + "/" + encodeURIComponent(id), values
      );
    },
    remove: function (model, id) {
      return request(
        "DELETE", "/data/" + encodeURIComponent(model) + "/" + encodeURIComponent(id)
      );
    },

    /**
     * aichip's own data, if this app has been granted it.
     *
     * Each of these rejects with an AichipNeedsScope error when the permission
     * has not been given, which is worth catching and showing rather than
     * treating as a crash.
     */
    api: {
      projects: function () { return request("GET", "/api/projects"); },
      tasks: function (opts) { return request("GET", "/api/tasks" + query(opts)); },
      runs: function (opts) { return request("GET", "/api/runs" + query(opts)); },
      spend: function (opts) { return request("GET", "/api/spend" + query(opts)); },
      agents: function () { return request("GET", "/api/agents"); },
      kbPages: function (opts) { return request("GET", "/api/kb/pages" + query(opts)); },
      createTask: function (task) { return request("POST", "/api/tasks", task); },
    },
  };
})();
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_named_method_is_actually_defined() {
        // The list is what the scaffold prompt is checked against, so a name
        // in it that the script does not define would send an agent looking
        // for something that is not there.
        for name in METHODS {
            assert!(
                CLIENT_JS.contains(&format!("{name}:")),
                "client.js never defines {name}"
            );
        }
    }

    #[test]
    fn the_header_is_on_every_request_by_construction() {
        // One place builds requests, and it sets the header. If a second one
        // ever appears, this is the test that should start failing.
        assert_eq!(CLIENT_JS.matches("fetch(").count(), 1);
        assert!(CLIENT_JS.contains("\"X-Aichip-App\": \"1\""));
    }

    #[test]
    fn a_missing_permission_is_its_own_kind_of_error() {
        // So an app can say "ask for this" rather than showing a stack trace.
        assert!(CLIENT_JS.contains("AichipNeedsScope"));
        assert!(CLIENT_JS.contains("needsScope"));
    }

    #[test]
    fn a_repeated_query_key_survives() {
        // `where` repeats, and keeping only the last one would silently return
        // the wrong rows rather than erroring.
        assert!(CLIENT_JS.contains("Array.isArray(value)"));
    }
}
