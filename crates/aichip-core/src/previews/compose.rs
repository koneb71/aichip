//! Preview a stack, not just a container.
//!
//! Most real projects are more than one process — a frontend, an API, a
//! database — and a preview that builds only the Dockerfile at the root shows
//! you a front end talking to nothing.
//!
//! ## The two things that make this safe
//!
//! **Ports are stripped and republished.** A compose file declares host ports:
//! `"${FRONTEND_PORT:-9000}:80"`, `"5173:5173"`. Bringing one up as written
//! seizes exactly the ports the user's own standing stacks run on — which is
//! the collision this whole feature exists to avoid. So every `ports:` entry is
//! removed and one service is republished on a free loopback port. Compose
//! *merges* port lists when you layer an override file, so removal has to
//! happen by rewriting the file; an override cannot take a binding away.
//!
//! **Everything is namespaced.** Compose prefixes networks, volumes and
//! container names with its project name, so running under
//! `aichip-preview-<id>` means a preview's `app` network and `backend-data`
//! volume are its own — it cannot join, reuse, or later delete the ones the
//! user's real stack is using.
//!
//! Pure and tested: the rewriting is where a mistake would be expensive, and it
//! is decided entirely from the file's text.

use serde_yaml::Value;

/// Compose file names, in the order Compose itself looks for them.
pub const COMPOSE_FILES: &[&str] = &[
    "compose.yaml",
    "compose.yml",
    "docker-compose.yaml",
    "docker-compose.yml",
];

/// Service names that usually mean "the thing a person opens".
const WEBBISH: &[&str] = &["web", "frontend", "www", "ui", "client", "app", "nginx"];

/// What we decided to do with a stack.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    /// The service whose port gets published.
    pub service: String,
    /// The port *inside* that service's container.
    pub container_port: u16,
    /// Nothing said which port; we guessed. Travels so the UI can admit it.
    pub port_assumed: bool,
    /// Every service the stack will start, for showing what is about to run.
    pub services: Vec<String>,
    /// The parsed file with every host binding removed. Rendered by
    /// [`render`], which puts exactly one binding back.
    doc: Value,
}

impl Plan {
    /// The final compose file: no declared bindings, plus one loopback
    /// publication of the service we chose.
    ///
    /// The binding is written into the file rather than passed as a flag
    /// because `docker compose up` has no `--publish`. That is also why the
    /// port has to be chosen before the file is written.
    pub fn render(&self, host_port: u16) -> String {
        let mut doc = self.doc.clone();
        if let Some(spec) = doc
            .get_mut("services")
            .and_then(Value::as_mapping_mut)
            .and_then(|m| m.get_mut(Value::String(self.service.clone())))
            .and_then(Value::as_mapping_mut)
        {
            spec.insert(
                Value::String("ports".into()),
                Value::Sequence(vec![Value::String(format!(
                    "127.0.0.1:{host_port}:{}",
                    self.container_port
                ))]),
            );
        }
        serde_yaml::to_string(&doc).unwrap_or_default()
    }

    /// The stripped file, for tests and for showing what will run.
    pub fn stripped(&self) -> String {
        serde_yaml::to_string(&self.doc).unwrap_or_default()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ComposeError {
    Unparseable(String),
    NoServices,
}

impl ComposeError {
    pub fn message(&self) -> String {
        match self {
            Self::Unparseable(why) => {
                format!("This project's compose file could not be read: {why}")
            }
            Self::NoServices => {
                "This project's compose file defines no services.".to_string()
            }
        }
    }
}

/// Read a stack and decide what to publish, stripping every host binding.
pub fn plan(text: &str) -> Result<Plan, ComposeError> {
    let mut doc: Value =
        serde_yaml::from_str(text).map_err(|e| ComposeError::Unparseable(e.to_string()))?;

    let services = doc
        .get_mut("services")
        .and_then(Value::as_mapping_mut)
        .ok_or(ComposeError::NoServices)?;
    if services.is_empty() {
        return Err(ComposeError::NoServices);
    }

    let names: Vec<String> = services
        .keys()
        .filter_map(|k| k.as_str().map(str::to_string))
        .collect();

    // Which service is the one you open, and on which port. Preference order:
    // a web-sounding name that publishes a port, then any service that
    // publishes one, then a web-sounding name at all, then the first service.
    let published: Vec<(String, u16)> = names
        .iter()
        .filter_map(|n| {
            container_port(services.get(Value::String(n.clone()))?).map(|p| (n.clone(), p))
        })
        .collect();

    let (service, container_port, port_assumed) = published
        .iter()
        .find(|(n, _)| is_webbish(n))
        .or_else(|| published.first())
        .map(|(n, p)| (n.clone(), *p, false))
        .unwrap_or_else(|| {
            let n = names
                .iter()
                .find(|n| is_webbish(n))
                .unwrap_or(&names[0])
                .clone();
            (n, super::recipe::ASSUMED_PORT, true)
        });

    // Strip host bindings everywhere. Not just from the published service:
    // a database that declares "5432:5432" would collide with the user's own
    // Postgres just as surely as a frontend would.
    for (_, spec) in services.iter_mut() {
        if let Some(map) = spec.as_mapping_mut() {
            map.remove(Value::String("ports".into()));
        }
    }

    Ok(Plan {
        service,
        container_port,
        port_assumed,
        services: names,
        doc,
    })
}

fn is_webbish(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    WEBBISH.iter().any(|w| lower == *w || lower.contains(w))
}

/// The container-side port a service publishes or exposes.
///
/// `"9000:80"` is host 9000 to container 80, so 80 is what we republish. The
/// host side is discarded on purpose — it is the number that would collide.
fn container_port(spec: &Value) -> Option<u16> {
    let map = spec.as_mapping()?;
    if let Some(ports) = map.get(Value::String("ports".into())).and_then(Value::as_sequence) {
        for entry in ports {
            if let Some(p) = port_of_entry(entry) {
                return Some(p);
            }
        }
    }
    // `expose:` publishes nothing but does say which port the service serves.
    map.get(Value::String("expose".into()))
        .and_then(Value::as_sequence)?
        .iter()
        .find_map(|e| parse_port(&scalar(e)?))
}

fn port_of_entry(entry: &Value) -> Option<u16> {
    // Long form: `{ target: 80, published: 9000 }`.
    if let Some(map) = entry.as_mapping() {
        return map
            .get(Value::String("target".into()))
            .and_then(|t| parse_port(&scalar(t)?));
    }
    // Short form: "9000:80", "127.0.0.1:9000:80", "80", "9000:80/tcp".
    let text = scalar(entry)?;
    let text = text.split('/').next().unwrap_or(&text).to_string();
    let parts: Vec<&str> = text.split(':').collect();
    // The container port is always last.
    parse_port(parts.last()?)
}

fn scalar(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// A literal port. `${WEB_PORT:-8080}` is not one — it resolves at compose
/// time, and guessing what it will become is how you publish the wrong thing.
fn parse_port(text: &str) -> Option<u16> {
    let t = text.trim();
    if t.contains('$') {
        return None;
    }
    t.parse::<u16>().ok().filter(|p| *p != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The user's own `windows11`, trimmed — the file this has to get right.
    const REAL: &str = r#"
services:
  backend:
    build: { context: ./backend }
    volumes: [backend-data:/data]
    networks: [app]
  frontend:
    build: { context: ., dockerfile: Dockerfile }
    ports:
      - "${FRONTEND_PORT:-9000}:80"
    networks: [app]
networks:
  app: { driver: bridge }
volumes:
  backend-data:
"#;

    #[test]
    fn publishes_the_web_service_on_its_container_port() {
        let p = plan(REAL).unwrap();
        assert_eq!(p.service, "frontend");
        // 80, not 9000. The host number is the one that would collide.
        assert_eq!(p.container_port, 80);
        assert!(!p.port_assumed);
        assert_eq!(p.services, vec!["backend", "frontend"]);
    }

    #[test]
    fn strips_every_host_binding_not_just_the_published_one() {
        // The regression that matters: bringing this up as written seizes 9000
        // and 5432, which are the ports the user's real stacks are on.
        let p = plan(
            r#"
services:
  web:
    ports: ["9000:80"]
  db:
    ports: ["5432:5432"]
"#,
        )
        .unwrap();
        let out = p.stripped();
        assert!(!out.contains("9000"), "{out}");
        assert!(!out.contains("5432"), "{out}");
        assert!(!out.contains("ports"), "{out}");
        // ...and the services themselves survive the surgery.
        assert!(out.contains("web"));
        assert!(out.contains("db"));
    }

    #[test]
    fn renders_exactly_one_loopback_binding_back() {
        let p = plan(REAL).unwrap();
        let out = p.render(54321);
        // One binding, on loopback, for the service we chose.
        assert!(out.contains("127.0.0.1:54321:80"), "{out}");
        assert_eq!(out.matches("127.0.0.1:").count(), 1, "{out}");
        // And the one it replaced is gone.
        assert!(!out.contains("9000"), "{out}");
        assert!(!out.contains("FRONTEND_PORT"), "{out}");
    }

    #[test]
    fn reads_every_shape_compose_allows_for_a_port() {
        let cases = [
            (r#"services: {web: {ports: ["80"]}}"#, 80),
            (r#"services: {web: {ports: ["9000:80"]}}"#, 80),
            (r#"services: {web: {ports: ["127.0.0.1:9000:80"]}}"#, 80),
            (r#"services: {web: {ports: ["9000:80/tcp"]}}"#, 80),
            (r#"services: {web: {ports: [{target: 8080, published: 9000}]}}"#, 8080),
            (r#"services: {web: {expose: [3000]}}"#, 3000),
        ];
        for (yaml, want) in cases {
            let p = plan(yaml).unwrap_or_else(|e| panic!("{yaml}: {e:?}"));
            assert_eq!(p.container_port, want, "{yaml}");
            assert!(!p.port_assumed, "{yaml}");
        }
    }

    #[test]
    fn an_interpolated_port_is_not_a_port() {
        // `${WEB_PORT:-8080}:80` still yields 80 — the container side is
        // literal. But when the *container* side is a variable there is nothing
        // honest to read, so it falls through to the guess.
        let p = plan(r#"services: {web: {ports: ["${WEB_PORT:-8080}:80"]}}"#).unwrap();
        assert_eq!(p.container_port, 80);

        let p = plan(r#"services: {web: {ports: ["8080:${PORT}"]}}"#).unwrap();
        assert!(p.port_assumed);
        assert_eq!(p.container_port, super::super::recipe::ASSUMED_PORT);
    }

    #[test]
    fn prefers_a_web_sounding_service_over_whichever_came_first() {
        let p = plan(
            r#"
services:
  api: {ports: ["8000:8000"]}
  frontend: {ports: ["9000:80"]}
"#,
        )
        .unwrap();
        assert_eq!(p.service, "frontend");
        // With no web-sounding name, the first that publishes anything wins.
        let p = plan(r#"services: {api: {ports: ["8000:8000"]}, worker: {}}"#).unwrap();
        assert_eq!(p.service, "api");
    }

    #[test]
    fn a_stack_that_publishes_nothing_still_gets_a_guess() {
        let p = plan(r#"services: {web: {image: nginx}, db: {image: postgres}}"#).unwrap();
        assert_eq!(p.service, "web");
        assert!(p.port_assumed);
    }

    #[test]
    fn refuses_what_it_cannot_read() {
        assert!(matches!(plan("services:"), Err(ComposeError::NoServices)));
        assert!(matches!(plan("services: {}"), Err(ComposeError::NoServices)));
        assert!(matches!(plan("name: x"), Err(ComposeError::NoServices)));
        assert!(matches!(
            plan("services: {web: {ports: [\"80\"]}"),
            Err(ComposeError::Unparseable(_))
        ));
    }
}
