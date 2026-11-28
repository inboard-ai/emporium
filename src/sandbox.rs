//! The limits every extension runs under.
//!
//! An extension gets no filesystem and no environment, a bounded amount of
//! linear memory, a fuel budget and a wall-clock deadline for each host
//! call, and outgoing HTTP only when its manifest declares the `network`
//! capability.
//!
//! Fuel is consumed as the guest executes and yields to the host at a fixed
//! interval, so a guest stuck in a loop still lets the per-call deadline
//! fire. A call that runs out of fuel traps and reports an error; a call
//! that outlives its deadline stops the worker, and later calls report
//! [`Error::ExtensionCrashed`](crate::Error::ExtensionCrashed).
//!
//! A host can send requests for named hosts somewhere else with [`Routes`]:
//! a test points an extension's API host at a server in its own process.
//! A host can also answer a named host's requests itself with an [`Answer`],
//! so an extension reaches a service the host speaks to on its behalf
//! without the extension holding that service's credential.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use http::uri::{Authority, Scheme};
use http_body_util::BodyExt;

use wasmtime::{Config, Store, StoreLimits, StoreLimitsBuilder};
use wasmtime_wasi_http::{RequestOptions, WasiBody, WasiHttpHooks};

use crate::Error;
use crate::manifest::Manifest;

/// The manifest capability that grants outgoing HTTP.
pub const NETWORK: &str = "network";

/// Resource limits for one extension instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    memory: usize,
    fuel: u64,
    deadline: Duration,
}

impl Default for Limits {
    /// 256 MiB of memory, about ten billion instructions and 60 seconds per
    /// call: far above what a connector spends waiting on its API, far below
    /// what lets a runaway guest hang the host.
    fn default() -> Self {
        Self {
            memory: 256 * 1024 * 1024,
            fuel: 10_000_000_000,
            deadline: Duration::from_secs(60),
        }
    }
}

impl Limits {
    /// The most linear memory, in bytes, the instance may grow to.
    pub fn memory(self, bytes: usize) -> Self {
        Self { memory: bytes, ..self }
    }

    /// The fuel each call starts with.
    pub fn fuel(self, fuel: u64) -> Self {
        Self { fuel, ..self }
    }

    /// The wall-clock time each call may take.
    pub fn deadline(self, deadline: Duration) -> Self {
        Self { deadline, ..self }
    }
}

/// How often, in fuel units, a running guest yields to the host executor.
const YIELD_INTERVAL: u64 = 1_000_000;

/// A host's answer to an extension's request for a named host, made in the
/// host's own process instead of over the network.
///
/// The request arrives with its whole body read, and the answer goes back to
/// the extension as the response it receives.
pub trait Answer: Send + Sync {
    /// Answer `request`.
    fn answer(&self, request: http::Request<Bytes>) -> Pin<Box<dyn Future<Output = http::Response<Bytes>> + Send>>;
}

/// Where requests for named hosts go instead of the hosts themselves.
///
/// Each route replaces a request's scheme and authority and keeps its path
/// and query, so an extension that always calls `https://<org host>/…`
/// reaches `http://127.0.0.1:<port>/…` when its host is routed there. A host
/// can instead answer a named host itself ([`Routes::answered`]). The
/// network grant still applies: a routed or answered request from an
/// extension without it is denied.
#[derive(Clone, Default)]
pub struct Routes(Arc<Vec<Route>>);

impl std::fmt::Debug for Routes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.0.iter().map(|route| &route.host)).finish()
    }
}

#[derive(Clone)]
struct Route {
    host: String,
    target: Target,
}

/// Where a routed host's requests go.
#[derive(Clone)]
enum Target {
    /// Another origin, over the network.
    Origin { scheme: Scheme, authority: Authority },
    /// The host's own answer.
    Answered(Arc<dyn Answer>),
}

impl Routes {
    /// Also send requests for `host` to `origin`, an `http` or `https` URL
    /// with no path, such as `http://127.0.0.1:8080`.
    pub fn to(self, host: impl Into<String>, origin: &str) -> Result<Self, Error> {
        let invalid = || Error::InvalidRoute(origin.to_owned());
        let uri: http::Uri = origin.parse().map_err(|_| invalid())?;
        let parts = uri.into_parts();
        let scheme = parts
            .scheme
            .filter(|s| *s == Scheme::HTTP || *s == Scheme::HTTPS)
            .ok_or_else(invalid)?;
        let authority = parts.authority.ok_or_else(invalid)?;
        if parts
            .path_and_query
            .is_some_and(|path| !matches!(path.as_str(), "" | "/"))
        {
            return Err(invalid());
        }
        Ok(self.with(host.into(), Target::Origin { scheme, authority }))
    }

    /// Also answer requests for `host` with `answer`, in this process.
    pub fn answered(self, host: impl Into<String>, answer: Arc<dyn Answer>) -> Self {
        self.with(host.into(), Target::Answered(answer))
    }

    fn with(self, host: String, target: Target) -> Self {
        let mut routes = Arc::unwrap_or_clone(self.0);
        routes.push(Route {
            host: host.to_ascii_lowercase(),
            target,
        });
        Self(Arc::new(routes))
    }

    /// Where `request`'s host is routed, if anywhere.
    fn target<B>(&self, request: &http::Request<B>) -> Option<&Target> {
        let host = request.uri().host()?;
        self.0
            .iter()
            .find(|route| route.host.eq_ignore_ascii_case(host))
            .map(|route| &route.target)
    }

    /// `request` sent where its host is routed, or unchanged. An answered
    /// host keeps its address, since no request for it leaves the process.
    fn apply<B>(&self, mut request: http::Request<B>) -> http::Request<B> {
        let Some(Target::Origin { scheme, authority }) = self.target(&request) else {
            return request;
        };
        let mut parts = request.uri().clone().into_parts();
        parts.scheme = Some(scheme.clone());
        parts.authority = Some(authority.clone());
        match http::Uri::from_parts(parts) {
            Ok(uri) => *request.uri_mut() = uri,
            Err(error) => tracing::warn!(%error, "a routed request kept its address"),
        }
        request
    }
}

/// The limits and grants for one extension, derived from its manifest.
#[derive(Debug, Clone)]
pub(crate) struct Sandbox {
    limits: Limits,
    network: bool,
    routes: Routes,
}

impl Sandbox {
    /// Grant what `manifest` declares, under `limits`.
    pub(crate) fn new(manifest: &Manifest, limits: Limits) -> Self {
        Self {
            limits,
            network: manifest.capabilities.get(NETWORK).copied().unwrap_or(false),
            routes: Routes::default(),
        }
    }

    /// Send requests for routed hosts where `routes` says.
    pub(crate) fn routed(self, routes: Routes) -> Self {
        Self { routes, ..self }
    }

    /// Turn on the engine features the limits rely on.
    pub(crate) fn configure(&self, config: &mut Config) {
        config.consume_fuel(true);
    }

    /// The per-store guard: the memory limiter and the network grant.
    pub(crate) fn guard(&self) -> Guard {
        Guard {
            limits: StoreLimitsBuilder::new()
                .memory_size(self.limits.memory)
                .trap_on_grow_failure(true)
                .build(),
            network: Network {
                allowed: self.network,
                routes: self.routes.clone(),
            },
        }
    }

    /// Install the guard's limiter on `store` and give it its first fuel.
    pub(crate) fn arm<T: AsMut<Guard>>(&self, store: &mut Store<T>) -> Result<(), Error> {
        store.limiter(|state| &mut state.as_mut().limits);
        store.fuel_async_yield_interval(Some(YIELD_INTERVAL))?;
        self.refuel(store)
    }

    /// Give `store` a full budget before the next call.
    pub(crate) fn refuel<T>(&self, store: &mut Store<T>) -> Result<(), Error> {
        store.set_fuel(self.limits.fuel)?;
        Ok(())
    }

    /// Run one call against its deadline. `Err` means the deadline passed
    /// and the instance must not be entered again.
    pub(crate) async fn call<F: Future<Output = ()>>(&self, call: F) -> Result<(), Elapsed> {
        tokio::time::timeout(self.limits.deadline, call)
            .await
            .map_err(|_| Elapsed(self.limits.deadline))
    }
}

/// A call outlived its deadline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Elapsed(pub(crate) Duration);

/// Per-store state the sandbox owns.
pub(crate) struct Guard {
    limits: StoreLimits,
    network: Network,
}

impl Guard {
    /// The HTTP hooks that enforce the network grant.
    pub(crate) fn network(&mut self) -> &mut Network {
        &mut self.network
    }
}

/// WASI-HTTP hooks that send requests only for extensions granted network,
/// where the host's routes say.
pub(crate) struct Network {
    allowed: bool,
    routes: Routes,
}

type Sent = Box<
    dyn Future<
            Output = wasmtime_wasi_http::Result<(
                http::Response<WasiBody>,
                Box<dyn Future<Output = wasmtime_wasi_http::Result<()>> + Send>,
            )>,
        > + Send,
>;

impl WasiHttpHooks for Network {
    fn send_request(
        &mut self,
        request: http::Request<WasiBody>,
        options: Option<RequestOptions>,
        fut: Box<dyn Future<Output = wasmtime_wasi_http::Result<()>> + Send>,
    ) -> Sent {
        if self.allowed {
            if let Some(Target::Answered(answer)) = self.routes.target(&request) {
                return answered(answer.clone(), request);
            }
            let request = self.routes.apply(request);
            return wasmtime_wasi_http::default_hooks().send_request(request, options, fut);
        }
        tracing::warn!(
            host = request.uri().host().unwrap_or_default(),
            "extension without the network capability tried to send a request"
        );
        Box::new(async { Err(wasmtime_wasi_http::Error::HttpRequestDenied) })
    }
}

/// `request`, read whole, answered by `answer` in this process.
fn answered(answer: Arc<dyn Answer>, request: http::Request<WasiBody>) -> Sent {
    Box::new(async move {
        let (parts, body) = request.into_parts();
        let body = body.collect().await?.to_bytes();
        let response = answer.answer(http::Request::from_parts(parts, body)).await;
        let response = response.map(|body| {
            http_body_util::Full::new(body)
                .map_err(|never| match never {})
                .boxed_unsync()
        });
        let io: Box<dyn Future<Output = wasmtime_wasi_http::Result<()>> + Send> = Box::new(async { Ok(()) });
        Ok((response, io))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasmtime::{Engine, Instance, Module};

    struct Host(Guard);

    impl AsMut<Guard> for Host {
        fn as_mut(&mut self) -> &mut Guard {
            &mut self.0
        }
    }

    const GUEST: &str = r#"
        (module
          (memory 1)
          (func (export "spin") (loop br 0))
          (func (export "grow") (result i32) (memory.grow (i32.const 16384)))
          (func (export "small") (result i32) (memory.grow (i32.const 1))))
    "#;

    fn manifest(network: Option<bool>) -> Manifest {
        let capabilities = network
            .map(|n| format!("[capabilities]\nnetwork = {n}\n"))
            .unwrap_or_default();
        let toml = format!(
            "[extension]\nid = \"test\"\nname = \"Test\"\nversion = \"0.1.0\"\nauthor = \"Test\"\nlicense = \"MIT\"\n\n[component]\nentry = \"x.wasm\"\n\n[config]\nschema = '{{}}'\n\n{capabilities}"
        );
        toml::from_str::<crate::manifest::RawManifest>(&toml)
            .unwrap()
            .into_manifest()
            .unwrap()
    }

    async fn instance(sandbox: &Sandbox) -> (Store<Host>, Instance) {
        let mut config = Config::new();
        sandbox.configure(&mut config);
        let engine = Engine::new(&config).unwrap();
        let module = Module::new(&engine, GUEST).unwrap();
        let mut store = Store::new(&engine, Host(sandbox.guard()));
        sandbox.arm(&mut store).unwrap();
        let instance = Instance::new_async(&mut store, &module, &[]).await.unwrap();
        (store, instance)
    }

    #[tokio::test]
    async fn a_spinning_guest_runs_out_of_fuel() {
        let sandbox = Sandbox::new(&manifest(None), Limits::default().fuel(5_000_000));
        let (mut store, instance) = instance(&sandbox).await;
        let spin = instance.get_typed_func::<(), ()>(&mut store, "spin").unwrap();
        let error = spin.call_async(&mut store, ()).await.unwrap_err();
        assert_eq!(error.downcast_ref::<wasmtime::Trap>(), Some(&wasmtime::Trap::OutOfFuel));
    }

    #[tokio::test]
    async fn a_spinning_guest_meets_its_deadline() {
        let limits = Limits::default().fuel(u64::MAX).deadline(Duration::from_millis(50));
        let sandbox = Sandbox::new(&manifest(None), limits);
        let (mut store, instance) = instance(&sandbox).await;
        let spin = instance.get_typed_func::<(), ()>(&mut store, "spin").unwrap();
        let outcome = sandbox
            .call(async {
                let _ = spin.call_async(&mut store, ()).await;
            })
            .await;
        assert_eq!(outcome, Err(Elapsed(Duration::from_millis(50))));
    }

    #[tokio::test]
    async fn memory_cannot_grow_past_the_limit() {
        let sandbox = Sandbox::new(&manifest(None), Limits::default().memory(2 * 65536));
        let (mut store, instance) = instance(&sandbox).await;
        let small = instance.get_typed_func::<(), i32>(&mut store, "small").unwrap();
        assert_eq!(
            small.call_async(&mut store, ()).await.unwrap(),
            1,
            "growing within the limit works"
        );
        let grow = instance.get_typed_func::<(), i32>(&mut store, "grow").unwrap();
        assert!(
            grow.call_async(&mut store, ()).await.is_err(),
            "growing past the limit traps"
        );
    }

    #[tokio::test]
    async fn refuel_restores_the_budget() {
        let sandbox = Sandbox::new(&manifest(None), Limits::default().fuel(1_000));
        let (mut store, _instance) = instance(&sandbox).await;
        store.set_fuel(0).unwrap();
        sandbox.refuel(&mut store).unwrap();
        assert_eq!(store.get_fuel().unwrap(), 1_000);
    }

    #[test]
    fn network_follows_the_manifest() {
        assert!(Sandbox::new(&manifest(Some(true)), Limits::default()).network);
        assert!(!Sandbox::new(&manifest(Some(false)), Limits::default()).network);
        assert!(
            !Sandbox::new(&manifest(None), Limits::default()).network,
            "undeclared means denied"
        );
    }

    #[tokio::test]
    async fn requests_without_the_network_grant_are_denied() {
        let mut network = Network {
            allowed: false,
            routes: Routes::default().to("example.com", "http://127.0.0.1:9").unwrap(),
        };
        let request = http::Request::builder()
            .uri("https://example.com/")
            .body(WasiBody::default())
            .unwrap();
        let sent = Box::into_pin(network.send_request(request, None, Box::new(async { Ok(()) })));
        assert!(matches!(sent.await, Err(wasmtime_wasi_http::Error::HttpRequestDenied)));
    }

    #[test]
    fn routes_send_a_host_elsewhere_and_keep_the_path() {
        let routes = Routes::default()
            .to("Org.My.Salesforce.com", "http://127.0.0.1:8080")
            .unwrap();
        let request = |uri: &str| http::Request::builder().uri(uri).body(()).unwrap();
        let routed = routes.apply(request(
            "https://org.my.salesforce.com/services/data/v62.0/query?q=SELECT+Id",
        ));
        assert_eq!(
            routed.uri().to_string(),
            "http://127.0.0.1:8080/services/data/v62.0/query?q=SELECT+Id"
        );
        let other = routes.apply(request("https://login.salesforce.com/services/oauth2/token"));
        assert_eq!(
            other.uri().to_string(),
            "https://login.salesforce.com/services/oauth2/token"
        );
        for origin in ["127.0.0.1:8080", "ftp://127.0.0.1", "http://127.0.0.1/api", "not a url"] {
            assert!(
                Routes::default().to("x.test", origin).is_err(),
                "{origin} is not an origin"
            );
        }
    }

    /// A granted extension's request for a routed host reaches a server in
    /// this process over plain HTTP, though it asked for HTTPS.
    #[tokio::test]
    async fn a_routed_request_reaches_a_local_server() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut seen = vec![0_u8; 4096];
            let read = stream.read(&mut seen).await.unwrap();
            let body = "{\"totalSize\":0}";
            let reply = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(reply.as_bytes()).await.unwrap();
            String::from_utf8_lossy(&seen[..read]).into_owned()
        });

        let mut network = Network {
            allowed: true,
            routes: Routes::default().to("org.my.salesforce.com", &origin).unwrap(),
        };
        let request = http::Request::builder()
            .uri("https://org.my.salesforce.com/services/data/v62.0/query?q=x")
            .body(WasiBody::default())
            .unwrap();
        let sent = Box::into_pin(network.send_request(request, None, Box::new(async { Ok(()) })));
        let (response, _io) = sent.await.unwrap();
        assert_eq!(response.status(), 200);
        let seen = server.await.unwrap();
        assert!(
            seen.starts_with("GET /services/data/v62.0/query?q=x HTTP/1.1"),
            "{seen}"
        );
    }

    /// Echoes the path and body it was asked for.
    struct Echo;

    impl Answer for Echo {
        fn answer(&self, request: http::Request<Bytes>) -> Pin<Box<dyn Future<Output = http::Response<Bytes>> + Send>> {
            let reply = format!("{} {}", request.uri().path(), String::from_utf8_lossy(request.body()));
            Box::pin(async move { http::Response::builder().status(201).body(Bytes::from(reply)).unwrap() })
        }
    }

    /// An answered host's request never leaves the process: the host's
    /// answer reads its path and body, and the extension receives the reply.
    #[tokio::test]
    async fn an_answered_host_is_answered_in_process() {
        let mut network = Network {
            allowed: true,
            routes: Routes::default().answered("shared.invalid", Arc::new(Echo)),
        };
        let body = http_body_util::Full::new(Bytes::from_static(b"{\"limit\":5}"))
            .map_err(|never| match never {})
            .boxed_unsync();
        let request = http::Request::builder()
            .method("POST")
            .uri("https://shared.invalid/json/2/res.company/search_read")
            .body(body)
            .unwrap();
        let sent = Box::into_pin(network.send_request(request, None, Box::new(async { Ok(()) })));
        let (response, _io) = sent.await.unwrap();
        assert_eq!(response.status(), 201);
        let reply = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&reply[..], b"/json/2/res.company/search_read {\"limit\":5}");

        let mut denied = Network {
            allowed: false,
            routes: Routes::default().answered("shared.invalid", Arc::new(Echo)),
        };
        let request = http::Request::builder()
            .uri("https://shared.invalid/")
            .body(WasiBody::default())
            .unwrap();
        let sent = Box::into_pin(denied.send_request(request, None, Box::new(async { Ok(()) })));
        assert!(matches!(sent.await, Err(wasmtime_wasi_http::Error::HttpRequestDenied)));
    }
}
